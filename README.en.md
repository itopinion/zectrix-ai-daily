# ZECTRIX AI Daily

[简体中文](README.md) | **English**

A daily global AI news briefing that runs on **ZECTRIX Hub** and shows on the **Note 4** e-ink device. The browser page is where you choose sources, time, and language. The device is where you read and listen.

This repository is also a reference for third-party Hub apps. Parsing, dedupe, ranking, the swappable local model, the render/audio client, and the package contract each live in their own module and have tests.

The app never talks to the panel directly. Hub owns the device, app switching, and the shared speech environment. The installable package targets **Apple Silicon Macs**, the same first platform as [zectrix-vocabulary](https://github.com/itopinion/zectrix-vocabulary).

## Install

1. [Download and start ZECTRIX Hub](https://github.com/itopinion/zectrix-hub/releases), then connect a Note 4 from its local page.
2. On an Apple Silicon Mac, build `dist/zectrix-ai-daily-<version>-macos-aarch64.zip` with the commands below, or use a published ZIP of that name.
3. Install that ZIP from Hub and open “AI 日报”.
4. For speech, configure an existing Qwen-TTS model and Python environment as described below, or use Hub's Kokoro components. Reopen the app to apply the configuration. Reading works without speech.

**Same device requirement as the vocabulary app: ZECTRIX Note 4, official firmware 3.7.0 or newer, in local push mode.** Firmware steps are in the [official guide](https://wiki.zectrix.com/zh/software/firmware) (Chinese). This app ships no firmware and does not claim Note 4C support.

Allow macOS local network access for `zectrix-hub` when prompted. If the page opens but the device stays blank, check **System Settings → Privacy & Security → Local Network**.

## Use

Open the `http://127.0.0.1:<port>` URL the process prints. It listens on loopback only.

- Enable sources and set the daily refresh time (default **07:00** local), item count (default 12, allowed 5–20), language, speech, speed, and volume.
- **Refresh now** fetches roughly the last day, clusters duplicate stories, and keeps the top items.
- If [Ollama](https://ollama.com) is running and the configured model exists (default `qwen3`), each item is rewritten into a title, an about-three-sentence summary, and a speakable script. If the model is down, the run still succeeds: items are ranked by source weight, recency, and popularity, and the feed text is used as the summary.
- Switching between 中文 and English rewrites the saved day in the new language. It does not fetch again.
- The right-hand column is a 400×300 Note 4 preview. Up/Down select, Confirm opens, Back returns to the list. Continuous play reads from the current item onward; any key stops it.
- **Archive** lists digests on disk. You can read one in the browser, or press **Show on device**.

Device keys use the same commands as the on-screen buttons:

| Key | List | Detail | While playing |
| --- | --- | --- | --- |
| Up / Down | Move the cursor; the page advances every 5 items | Page the summary | Stop |
| Confirm, short | Open and read if speech is on | Return and stop | Stop |
| Back / Left / Cancel | — | Return to the list | Stop |

Note 4 uses long confirm for recording. This app only handles Short key events. Reopen a story to replay it; start continuous play from the web Play button.

## Architecture

```
sources (RSS/Atom, HN, arXiv)
        │  fetch
        ▼
   Article ── cluster ──► Cluster ── rank ──► heuristic score
        │                                      │
        │                            LlmBackend::complete
        │                                 │ on any failure: fallback
        ▼                                 ▼
   digests/YYYY-MM-DD.json ◄── Digest (title, summary, script, source clusters)
        │
        ├── loopback settings page (axum)
        └── layout.rs → POST /runtime/render → Note 4
                         script → workers/tts.py (Kokoro) → POST /runtime/audio
```

| Module | Role |
| --- | --- |
| `src/parse.rs` | RSS 2.0 and Atom. An HTML body is an error, so a dead “feed” URL cannot become a story. |
| `src/fetch.rs` | Concurrent fetch. One source failing is a warning. Hacker News uses the official Firebase API and keeps AI-looking titles. |
| `src/cluster.rs` | Same URL (tracking params stripped) or a close title becomes one story. |
| `src/rank.rs` | Importance without a model: source weight, recency, HN points, and how many outlets carried it. |
| `src/llm.rs` | The `LlmBackend` trait. The default is Ollama `POST /api/chat`. |
| `src/pipeline.rs` | Wires the steps. A language change calls `regenerate` and reads clusters from disk. |
| `src/device.rs` | List / detail / continuous-play state machine. No Hub required. |
| `src/layout.rs` | The 400×300 card JSON. |
| `src/hub.rs` | Hub runtime client. |
| `src/speech.rs` | JSON-lines speech worker, same shape as the vocabulary app. |
| `src/server.rs` | Loopback settings page, scheduler, and event loop. |
| `web/` | Settings page with no build step, compiled into the binary. |
| `sources.toml` | Default catalog. Drop a file of the same name in the data directory to override it without rebuilding. |

The scheduler lives in-process. If the app starts after today's slot and that date has no digest, it catches up once; otherwise it sleeps until the next local time. Tests set `ZECTRIX_DISABLE_SCHEDULER=1`.

## app.json and permissions

The `app.json` in the repository has an empty `files` map and **cannot be installed**. The packager fills SHA-256 hashes and writes them into the ZIP.

```json
{
  "id": "zectrix-ai-daily",
  "name": "AI 日报",
  "version": "0.1.0",
  "api_version": 1,
  "platforms": ["macos-aarch64"],
  "entry": "bin/zectrix-ai-daily",
  "capabilities": ["display", "keys", "audio", "speech"],
  "files": {}
}
```

`capabilities` is what this app asks Hub for: a display, keys, audio playback, and the shared speech environment. It does not record, so `recording` is absent.

Package contract (the same shape as the vocabulary app's `scripts/package-app.py`; the Hub repo itself was not readable here — see the assumptions at the end):

- `app.json` is the first ZIP entry.
- `files` maps every path except `app.json` to the SHA-256 hex of that file's bytes.
- `entry` is listed in `files`, with Unix mode `0755`. Other files are `0644`.
- Symlinks, `.env`, model weights, wav/mp3, and paths such as `data/`, `target/`, and `dev-data/` are rejected.
- A sibling `<zip-name>.sha256` is written next to the archive.

`scripts/verify-package.py` hashes the ZIP again and checks that contract.

## Calling Hub

Hub injects these when it launches the app:

| Variable | Role |
| --- | --- |
| `ZECTRIX_HUB_URL` | Must be an `http://127.0.0.1` or `http://localhost` origin: no userinfo, query, or path other than `/`. |
| `ZECTRIX_APP_TOKEN` | Bearer token for this process. It is never sent to the browser or returned by `/api/boot`. |
| `ZECTRIX_APP_DATA` | Writable directory. App updates leave it in place. |
| `ZECTRIX_APP_ROOT` | Install directory, used to find `sources.toml` and `workers/tts.py`. |
| `ZECTRIX_APP_PORT` / `ZECTRIX_AI_DAILY_PORT` / `ZECTRIX_VOCAB_PORT` | Settings-page port, checked in this order. Verified against Hub v1 startup code: it currently injects `ZECTRIX_VOCAB_PORT` for every app. |
| `ZECTRIX_TTS_PYTHON` | Optional path to Hub's speech Python. When unset, speech reports that it is not installed and reading still works. |

Every Hub call sends `Authorization: Bearer <ZECTRIX_APP_TOKEN>` and does not follow redirects.

### `GET /runtime/status`

```json
{ "active": true, "lease": 3, "device": { "connected": true } }
```

`active: false` means another app is in front. Device keys and the web device controls are rejected; refreshing a digest is still allowed. A lease change stops playback.

### `GET /runtime/events?after=<cursor>`

```json
{
  "cursor": 4,
  "events": [
    { "lease": 3, "event": { "type": "key", "key": "Down", "action": "Short" } }
  ]
}
```

Events whose `lease` is not the current one are ignored. Handled `type` values:

- `key` with `key` of `Up`, `Down`, `Confirm`, `Back`, `Left`, `Cancel`, or `Return`.
- `disconnected`, which stops speech.

### `POST /runtime/render`

```json
{
  "layout": {
    "canvas": { "w": 400, "h": 300 },
    "elements": [
      { "type": "rect", "x": 0, "y": 0, "w": 399, "h": 299 },
      { "type": "hline", "x": 8, "y": 34, "w": 384 },
      {
        "type": "text",
        "content": "AI DAILY",
        "x": 16, "y": 18, "size": 16,
        "width_limit": 160, "align": "left",
        "weight": "bold", "font": "default"
      }
    ]
  },
  "lease": 3,
  "present": true
}
```

The body of the response is a PNG. `present: false` builds a preview without pushing it (the vocabulary app does this while inactive). This app treats `y` as the vertical center of a text run and wraps lines itself. The outer `rect` is used as a stroke, not a filled highlight, because the only `rect` in the vocabulary layout is the screen frame.

The list shows 5 items per page (title and source). The detail page shows title, source, time, and a paged summary.

### `POST /runtime/audio`

- Body: 16 kHz, mono, 16-bit PCM WAV.
- Headers: `Content-Type: audio/wav`, `X-Lease`, `X-Volume` (0–100).
- On success, JSON `result` must be `Ok(Completed)`. Playback is assumed to block until then, which is what lets continuous play wait for each item.

### `POST /runtime/stop`

```json
{ "lease": 3 }
```

Called when any key interrupts continuous play, so the current clip can be cut off.

The settings page uses the same local guards as the vocabulary app: `Host` must be `127.0.0.1:<port>` or `localhost:<port>`, `Origin` must match, and non-GET requests need `X-App-Token` (a per-process page token, not the Hub token). Responses send `nosniff`, `no-store`, and a tight `Content-Security-Policy`.

## Swappable local model

```rust
#[async_trait]
pub trait LlmBackend: Send + Sync {
    fn id(&self) -> &str;
    fn model_name(&self) -> &str;
    async fn complete(&self, request: &LlmRequest) -> Result<String>;
}
```

`OllamaBackend` turns clusters into a prompt and calls `POST {ollama_url}/api/chat` with `stream: false`, `think: false`, and `format: json`. The URL may only be loopback `http://127.0.0.1` or `http://localhost`, so the settings page cannot be used as an open proxy.

The model should return:

```json
{
  "items": [
    { "id": "<cluster id>", "title": "", "summary": "", "tts": "", "importance": 80 }
  ]
}
```

`<think>` blocks and Markdown fences are stripped. If parsing, the timeout, or the connection fails, `compose` keeps the feed summaries and records why in `warnings`. To use another runtime (llama.cpp, MLX, or another machine you trust), implement the trait and return it from `pipeline::backend_for`.

Without a model, a language switch changes UI strings and the spoken lead-in (`来自…` / `From …`) only. Summaries stay in the source language. The app does not pretend to translate.

## App data

Everything lives under `ZECTRIX_APP_DATA` and is not written back into the install package:

```
settings.json          language, schedule, count, speech, model, source switches
sources.toml           optional override of the bundled catalog
digests/YYYY-MM-DD.json
audio/<sha256>.wav     synthesized scripts
logs/tts.log
```

Each digest stores the clusters as well as the displayed items, so a language change works offline. Date arguments must match `YYYY-MM-DD`, which blocks path traversal.

## Default sources

Requested on 2026-10-14. Feeds that did not return a feed were left out of `sources.toml`.

| Source | URL | Result |
| --- | --- | --- |
| OpenAI | `https://openai.com/news/rss.xml` | RSS, ok |
| Google DeepMind | `https://deepmind.google/blog/rss.xml` | RSS, ok |
| Google AI Blog | `https://blog.google/innovation-and-ai/technology/ai/rss/` | RSS, ok |
| MIT Technology Review | `https://www.technologyreview.com/topic/artificial-intelligence/feed/` | RSS, ok |
| The Verge AI | `https://www.theverge.com/rss/ai-artificial-intelligence/index.xml` | Atom, ok |
| Hugging Face | `https://huggingface.co/blog/feed.xml` | RSS, ok |
| TechCrunch AI | `https://techcrunch.com/category/artificial-intelligence/feed/` | RSS, ok |
| 量子位 | `https://www.qbitai.com/feed` | RSS, ok |
| 机器之心 | `https://raw.githubusercontent.com/osnsyc/Wechat-Scholar/main/channels/gh_dbc0a5474692.xml` | Working public mirror. The official `https://www.jiqizhixin.com/rss` returns HTML and was dropped |
| Hacker News | `https://hacker-news.firebaseio.com/v0` | Official API. Stories whose titles look like AI news and fall inside the window |
| arXiv | `https://export.arxiv.org/api/query` | `cs.AI` and `cs.CL`, newest first, count capped |

Dropped:

- Anthropic: `/rss.xml`, `/news/rss.xml`, and the other paths tried returned 404. No public RSS/Atom feed was found. Coverage can still arrive via The Verge, TechCrunch, or Hacker News.
- Meta AI: `https://ai.meta.com/blog/rss/` returned 404.

The default window is about 30 hours (48 for arXiv, which publishes in a daily batch) so a morning edition still sees the previous evening.

## Develop, test, package

Install Rust stable (1.89 or newer). Packaging uses the Python 3 standard library. Hub renders text and talks to the device.

```sh
cargo test --locked
cargo build --release --locked
./scripts/package-app.sh          # installable ZIP only on macOS Apple Silicon
python3 scripts/verify-package.py dist/*.zip
```

On Linux or another architecture, a contract-check archive (not a Hub install package) is:

```sh
ZECTRIX_ALLOW_HOST_PACKAGE=1 ./scripts/package-app.sh
```

Settings page without Hub:

```sh
ZECTRIX_DEV=1 cargo run
# http://127.0.0.1:43117
```

Dev mode uses `./dev-data`, skips Hub, and draws the e-ink preview in the browser from the same view model the device would show.

Tests cover RSS/Atom/arXiv parsing, HTML rejected as a feed, the Hacker News filter, URL dedupe, title clustering, heuristic rank, model JSON (fences and `<think>`), the no-model fallback, settings validation, per-date archives, and an integration test against a fake Hub (refresh, lease isolation, up/down/confirm/back).

CI is `.github/workflows/ci.yml`: `cargo test --locked` on Ubuntu, then package-and-upload on macOS 15 Apple Silicon. It does not download models, touch a device, or publish a Release.

## Still to verify on a Mac and a Note 4

The public vocabulary app was readable. The private `itopinion/zectrix-hub` repository was **not** (this environment had no GitHub credentials). These points are assumptions until they are tried on Apple Silicon and a Note 4:

- The port name has been verified against Hub v1 source and tested with a mock Hub; the full installation flow still needs validation.
- Whether `/runtime/*` matches the vocabulary client exactly, in particular the audio `result` string, the long-press `action` value, and whether a Back key exists.
- Whether card `y` is the vertical center, the baseline, or the top. Coordinates follow the vocabulary layout and treat `y` as the center.
- Whether Hub's Kokoro environment can load Mandarin `lang_code=z` and voice `zf_xiaobei`. English defaults to `af_heart`, as in the vocabulary app.
- Whether `POST /runtime/stop` interrupts a `POST /runtime/audio` that has not returned yet.
- Whether a full-frame e-ink refresh is acceptable. Each state change submits a whole page.
- The full Hub install path for a `macos-aarch64` ZIP, including the message when speech components are missing.

## License

Project code is MIT. See `LICENSE`. Third-party components and models keep their own licenses; see `THIRD_PARTY_NOTICES.md`.

## Codex and local Qwen-TTS (0.1.3)

Choose Codex in the summary source selector to use the signed-in local Codex CLI.
It runs an ephemeral, read-only session with structured JSON output and uses the
existing account quota. No local text model is required. Ollama and feed-only
summaries remain available; existing settings keep their previous backend.

Store machine-specific paths in `local-runtime.json` in the app data directory,
then restart the app. Replace the example paths with actual absolute paths:

```json
{
  "codex_bin": "/absolute/path/to/codex",
  "tts_backend": "qwen",
  "tts_python": "/absolute/path/to/mlx-audio-environment/bin/python",
  "qwen_model": "/absolute/path/to/Qwen3-TTS-CustomVoice"
}
```

`codex_model` is optional. Qwen3-TTS CustomVoice weights must already exist locally;
the app neither downloads nor packages models. Qwen-ASR is a speech recognition
model and cannot generate speech. Chinese uses Vivian and English uses Ryan. The
worker produces 16 kHz mono PCM WAV; resampling for speed also changes pitch.

On Note 4, short Confirm opens and reads a story when speech is enabled; short
Confirm again returns and stops. Reopen to replay. Continuous play uses the web
button. Long press belongs to device recording and is ignored by this app.
Selection uses a black row with white text. Speech runs in the background so
navigation remains responsive during synthesis.
