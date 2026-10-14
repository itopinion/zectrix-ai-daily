"""Re-check a Hub ZIP against the manifest contract. See package-app.py."""
from pathlib import Path
import hashlib
import json
import stat
import sys
import zipfile

archive = Path(sys.argv[1])
sidecar = archive.with_suffix(".zip.sha256")
blob = archive.read_bytes()
digest = hashlib.sha256(blob).hexdigest()
if sidecar.exists():
    recorded = sidecar.read_text().split()[0]
    if recorded != digest:
        raise SystemExit(f"sidecar mismatch: {recorded} != {digest}")
with zipfile.ZipFile(archive) as zf:
    names = zf.namelist()
    if "app.json" not in names or names[0] != "app.json":
        raise SystemExit("app.json must be the first zip entry")
    manifest = json.loads(zf.read("app.json"))
    files = manifest["files"]
    if set(names) != {"app.json", *files}:
        raise SystemExit(f"zip entries != manifest files: {names}")
    entry = manifest["entry"]
    if entry not in files:
        raise SystemExit("entry missing from files")
    for name, expected in files.items():
        got = hashlib.sha256(zf.read(name)).hexdigest()
        if got != expected:
            raise SystemExit(f"hash mismatch for {name}")
    info = zf.getinfo(entry)
    mode = (info.external_attr >> 16) & 0o777
    if mode != 0o755:
        raise SystemExit(f"entry mode is {oct(mode)}, expected 0o755")
    for name in files:
        if name == entry:
            continue
        mode = (zf.getinfo(name).external_attr >> 16) & 0o777
        if mode != 0o644:
            raise SystemExit(f"{name} mode is {oct(mode)}")
    if stat.S_ISLNK(info.external_attr >> 16):
        raise SystemExit("entry is a symlink")
print(f"ok {archive.name} {digest}")
