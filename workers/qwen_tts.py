"""Offline JSON-lines bridge for an existing MLX Qwen3-TTS CustomVoice model."""
import contextlib
import json
import math
import os
import sys
import traceback
from pathlib import Path

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
os.environ["TOKENIZERS_PARALLELISM"] = "false"
wire = sys.stdout

try:
    model_path = Path(os.environ["ZECTRIX_QWEN_MODEL"]).resolve(strict=True)
    config = json.loads((model_path / "config.json").read_text())
    if config.get("model_type") != "qwen3_tts":
        raise ValueError("需要 Qwen3-TTS 语音生成模型，Qwen-ASR 识别模型不适用")
    if config.get("tts_model_type") != "custom_voice":
        raise ValueError("需要 Qwen3-TTS CustomVoice 模型")
    with contextlib.redirect_stdout(sys.stderr):
        import numpy as np
        import soundfile as sf
        from scipy.signal import resample_poly
        from mlx_audio.tts.utils import load_model
        model = load_model(str(model_path))
    print(json.dumps({"ready": True, "model": "qwen3-tts"}), file=wire, flush=True)
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
        language = "Chinese" if req.get("lang") in {"zh", "z"} else "English"
        speaker = "Vivian" if language == "Chinese" else "Ryan"
        speed = min(1.4, max(0.7, float(req.get("speed") or 1.0)))
        with contextlib.redirect_stdout(sys.stderr):
            chunks = list(model.generate_custom_voice(text=text, speaker=speaker, language=language, max_tokens=4096))
            if not chunks:
                raise ValueError("Qwen-TTS returned empty audio")
            pcm = np.concatenate([np.asarray(chunk.audio).reshape(-1) for chunk in chunks])
            source_rate = round(chunks[0].sample_rate * speed)
            divisor = math.gcd(16000, source_rate)
            pcm = resample_poly(pcm, 16000 // divisor, source_rate // divisor)
            dest = Path(req["path"])
            dest.parent.mkdir(parents=True, exist_ok=True)
            tmp = dest.with_suffix(".tmp.wav")
            sf.write(tmp, pcm, 16000, subtype="PCM_16")
            tmp.replace(dest)
        print(json.dumps({"ok": True, "path": str(dest), "samples": len(pcm), "voice": speaker, "lang": language}), file=wire, flush=True)
    except Exception as exc:
        traceback.print_exc(file=sys.stderr)
        print(json.dumps({"ok": False, "error": str(exc)}), file=wire, flush=True)
