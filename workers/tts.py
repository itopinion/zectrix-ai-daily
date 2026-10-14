"""JSON-lines Kokoro bridge for zectrix-ai-daily.

Hub owns the Python environment and the model cache. This process stays on
the loopback machine: the Rust app writes one JSON object per line and reads
one JSON object back. Weights are not shipped in the app package.

Voices (Kokoro-82M):
  en  af_heart   American English, lang_code a
  zh  zf_xiaobei Mandarin, lang_code z
"""
import os
import sys
import json
import contextlib
import traceback
from pathlib import Path

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TOKENIZERS_PARALLELISM"] = "false"
wire = sys.stdout
pipelines = {}


def lang_for(voice, requested):
    voice = voice or ""
    if voice.startswith(("zf", "zm")):
        return "z"
    if voice.startswith(("bf", "bm")):
        return "b"
    if voice.startswith(("af", "am")):
        return "a"
    if requested in {"z", "zh"}:
        return "z"
    return "a"


def pipeline_for(code):
    if code not in pipelines:
        with contextlib.redirect_stdout(sys.stderr):
            from kokoro import KPipeline
            pipelines[code] = KPipeline(lang_code=code, repo_id="hexgrad/Kokoro-82M", device="cpu")
    return pipelines[code]


try:
    with contextlib.redirect_stdout(sys.stderr):
        import numpy as np
        import soundfile as sf
        from scipy.signal import resample_poly
    print(json.dumps({"ready": True, "model": "kokoro"}), file=wire, flush=True)
except Exception as exc:
    traceback.print_exc(file=sys.stderr)
    print(json.dumps({"ready": False, "error": str(exc)}), file=wire, flush=True)
    sys.exit(1)

for line in sys.stdin:
    try:
        req = json.loads(line)
        text = (req.get("text") or "").strip()
        if not text:
            raise ValueError("empty text")
        voice = req.get("voice") or "af_heart"
        speed = float(req.get("speed") or 1.0)
        speed = min(1.4, max(0.7, speed))
        code = lang_for(voice, req.get("lang") or "en")
        with contextlib.redirect_stdout(sys.stderr):
            model = pipeline_for(code)
            chunks = [audio.numpy() for _, _, audio in model(text, voice=voice, speed=speed)]
            if not chunks:
                raise ValueError("Kokoro returned empty audio")
            audio = resample_poly(np.concatenate(chunks), 2, 3)
            dest = Path(req["path"])
            dest.parent.mkdir(parents=True, exist_ok=True)
            tmp = dest.with_suffix(".tmp.wav")
            sf.write(tmp, audio, 16000, subtype="PCM_16")
            tmp.replace(dest)
            out = {"path": str(dest), "samples": int(len(audio)), "voice": voice, "lang": code}
        print(json.dumps({"ok": True, **out}), file=wire, flush=True)
    except Exception as exc:
        traceback.print_exc(file=sys.stderr)
        print(json.dumps({"ok": False, "error": str(exc)}), file=wire, flush=True)
