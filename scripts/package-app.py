"""Developer-only ZIP builder. Hub installs the resulting file without Python.

The contract, mirrored from the vocabulary app because the Hub repo was not
readable here:

- ``app.json`` is the manifest (id, name, version, api_version, platforms,
  entry, capabilities, description, files).
- ``files`` maps every packaged path except ``app.json`` itself to the
  SHA-256 hex of its bytes.
- The entry binary is mode 0755; other files are 0644. Symlinks and a
  denylist (data, models, audio, secrets) are rejected.
- A sibling ``.zip.sha256`` file records the archive digest.
"""
from pathlib import Path
import hashlib
import json
import re
import stat
import sys
import zipfile

root = Path(sys.argv[1]).resolve()
dest = Path(sys.argv[2]).resolve()
platform = sys.argv[3]
manifest = json.loads((root / "app.json").read_text())
if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-.][A-Za-z0-9.-]+)?", manifest["version"]):
    raise SystemExit("Invalid app version")
if not re.fullmatch(r"[a-z0-9_]+(?:-[a-z0-9_]+)+", platform):
    raise SystemExit(f"Invalid platform {platform}")
manifest["platforms"] = [platform]
forbidden = {
    "data", ".git", "__pycache__", ".DS_Store", ".env", "recordings",
    "diagnostics", "models", "verification", "dev-data", "target", "dist",
}
files = {}
for file in sorted(root.rglob("*")):
    relative = file.relative_to(root)
    if any(part in forbidden for part in relative.parts) or file.is_symlink():
        raise SystemExit(f"Forbidden release path: {relative}")
    if not file.is_file():
        continue
    if file.suffix.lower() in {".wav", ".mp3", ".safetensors", ".pth", ".pt", ".pyc", ".bin", ".elf", ".hex", ".dfu"}:
        raise SystemExit(f"Recording/model/cache in release: {relative}")
    if relative.as_posix() != "app.json":
        files[relative.as_posix()] = hashlib.sha256(file.read_bytes()).hexdigest()
if manifest["entry"] not in files:
    raise SystemExit("Missing executable entry")
manifest["files"] = files
(root / "app.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
dest.mkdir(parents=True, exist_ok=True)
archive = dest / f"zectrix-ai-daily-{manifest['version']}-{platform}.zip"
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as out:
    for name in ["app.json", *files]:
        file = root / name
        info = zipfile.ZipInfo(name)
        info.create_system = 3
        mode = 0o755 if name == manifest["entry"] else 0o644
        info.external_attr = (stat.S_IFREG | mode) << 16
        info.compress_type = zipfile.ZIP_DEFLATED
        out.writestr(info, file.read_bytes())
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix(".zip.sha256").write_text(f"{digest}  {archive.name}\n")
print(f"应用包：{archive}")
