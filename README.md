# ZECTRIX AI 日报

**简体中文** | [English](README.en.md)

运行在 **ZECTRIX Hub** 上的每日全球人工智能新闻简报，画面显示在 **Note 4** 墨水屏上。网页用来设定信源、时间和语言；设备用来翻阅和收听。

本仓库同时是给第三方开发者的参考实现：解析、去重、排序、可替换的本地模型、Hub 渲染与音频、打包契约都放在独立模块里，并配有测试。

应用不直接连接硬件。设备连接、应用切换和语音环境由 Hub 管理。首版安装包的目标平台是 **Apple Silicon Mac**，与 [zectrix-vocabulary](https://github.com/itopinion/zectrix-vocabulary) 相同。

## 安装

1. [下载并启动 ZECTRIX Hub](https://github.com/itopinion/zectrix-hub/releases)，在本地管理页连接 Note 4。
2. 在 Apple Silicon Mac 上用下面的「开发与打包」生成 `dist/zectrix-ai-daily-<version>-macos-aarch64.zip`，或使用已发布的同名 ZIP。
3. 在 Hub 的应用安装入口选择该 ZIP，打开「AI 日报」。
4. 需要朗读时，按文末「本机 Qwen-TTS」配置已有模型和 Python 环境；也可使用 Hub 的 Kokoro 语音组件。重新打开本应用后生效。没有语音组件时，简报仍可阅读。

**设备要求与词汇应用相同：ZECTRIX Note 4，官方固件 3.7.0 及以上，并进入本地推送模式。** 固件升级见 [官方说明](https://wiki.zectrix.com/zh/software/firmware)。本应用不包含固件，也不自动兼容 Note 4C。

macOS 首次询问 `zectrix-hub` 的本地网络权限时需要允许。网页能开但设备无画面时，检查「系统设置 → 隐私与安全性 → 本地网络」。

## 使用

浏览器打开应用打印的 `http://127.0.0.1:<端口>`（只监听本机）。

- 勾选信源，设定每天的刷新时间（默认 **07:00** 本地时间）、保留条数（默认 12，允许 5–20）、语言、朗读开关、语速和音量。
- 「立即刷新」会抓取过去约一天的条目，去重后保留最重要的若干条。
- 若本机 [Ollama](https://ollama.com) 在运行且模型可用（默认 `qwen3`），每条会重写成标题、约三句摘要和一段可朗读的讲稿。连不上模型时不会失败：按信源权重、新旧和热度排序，摘要直接用信源正文。
- 切换中文 / English 会用当天已经保存的原文，按新语言重写简报，不需要重新抓取。
- 右侧是 Note 4 的 400×300 预览。上 / 下选择，确认打开，返回回到列表。连播从当前条开始按顺序朗读，任意键停止。
- 「往日简报」读取本机归档。可以只在网页里看，也可以「在设备上显示」某一天。

设备按键（与网页按钮同一套命令）：

| 按键 | 列表 | 详情 | 连播中 |
| --- | --- | --- | --- |
| 上 / 下 | 移动选择，满 5 条自动翻页 | 翻摘要页 | 停止 |
| 确认短按 | 打开当前条；若开启朗读则读这一条 | 返回列表并停止朗读 | 停止 |
| 返回 / Left / Cancel | — | 回到列表 | 停止 |

Note 4 长按确认会触发设备录音，因此本应用只处理 Short 短按。连播从网页按钮启动；重新进入详情可重听。

## 架构

```
信源 (RSS/Atom、HN、arXiv)
        │  fetch
        ▼
   文章 Article ── cluster ──► 聚类 Cluster ── rank ──► 启发式分数
        │                                              │
        │                                    LlmBackend::complete
        │                                         │ 失败则 fallback
        ▼                                         ▼
   digests/YYYY-MM-DD.json ◄── Digest（标题、三句摘要、讲稿、原文聚类）
        │
        ├── 网页设置页（本机 axum）
        └── layout.rs → POST /runtime/render → Note 4
                         讲稿 → workers/tts.py (Kokoro) → POST /runtime/audio
```

| 模块 | 职责 |
| --- | --- |
| `src/parse.rs` | RSS 2.0 与 Atom。HTML 页面会判为失败，避免把失效的「feed」当成新闻。 |
| `src/fetch.rs` | 并发抓取。单个信源失败只记警告。HN 走官方 Firebase API，并按标题过滤 AI。 |
| `src/cluster.rs` | 相同 URL（去掉 `utm_` 等参数）或标题足够接近，并成一条。 |
| `src/rank.rs` | 无模型时的重要性：信源权重、新旧、HN 分数、被多少家报道。 |
| `src/llm.rs` | `LlmBackend` trait。默认实现是 Ollama `POST /api/chat`。 |
| `src/pipeline.rs` | 把上面几步串起来。换语言走 `regenerate`，只读磁盘上的聚类。 |
| `src/device.rs` | 列表 / 详情 / 连播的按键状态机，不依赖 Hub。 |
| `src/layout.rs` | 400×300 卡片 JSON。 |
| `src/hub.rs` | Hub 运行时客户端。 |
| `src/speech.rs` | 与词汇应用相同的 JSON Lines 语音进程。 |
| `src/server.rs` | 只绑定 `127.0.0.1` 的设置页、定时任务和事件循环。 |
| `web/` | 无构建步骤的设置页，编译进二进制。 |
| `sources.toml` | 默认证源。放到数据目录同名文件即可覆盖，不用重新编译。 |

定时任务在进程内运行：启动时如果已经过了今天的刷新点且当天没有归档，会补跑一次；否则睡到下一次本地时间。测试里设置 `ZECTRIX_DISABLE_SCHEDULER=1` 可关掉。

## app.json 与权限

源码里的 `app.json` 是空 `files` 模板，**不能直接安装**。打包脚本会填上每个文件的 SHA-256，并写进 ZIP。

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

`capabilities` 表示本应用需要的 Hub 能力：画面、按键、播放音频、使用 Hub 的本地语音环境。不录音，所以没有 `recording`。

打包契约（与词汇应用的 `scripts/package-app.py` 一致；Hub 源码在本环境读不到，见文末假设）：

- ZIP 第一条是 `app.json`。
- `files` 的键是 ZIP 内除 `app.json` 以外的路径，值是该文件字节的 SHA-256 十六进制。
- `entry` 必须出现在 `files` 里，Unix 权限 `0755`，其他文件 `0644`。
- 拒绝符号链接、`.env`、模型权重、wav/mp3、以及 `data/`、`target/`、`dev-data/`。
- 旁边写一个 `<zip 文件名>.sha256`。

`scripts/verify-package.py` 会把 ZIP 再哈希一遍，用来核对这份契约。

## 调用 Hub

环境变量由 Hub 在启动应用时注入：

| 变量 | 作用 |
| --- | --- |
| `ZECTRIX_HUB_URL` | 必须是 `http://127.0.0.1` 或 `http://localhost` 的根地址，不能带用户名、查询串或其他路径。 |
| `ZECTRIX_APP_TOKEN` | Hub 发给本进程的 Bearer 令牌。不会出现在网页或 `/api/boot` 里。 |
| `ZECTRIX_APP_DATA` | 可写的数据目录，更新应用时保留。 |
| `ZECTRIX_APP_ROOT` | 安装目录，用来找 `sources.toml` 和 `workers/tts.py`。 |
| `ZECTRIX_APP_PORT` / `ZECTRIX_AI_DAILY_PORT` / `ZECTRIX_VOCAB_PORT` | 设置页端口，按此顺序读取。已核对 Hub v1 启动源码：当前向所有应用注入 `ZECTRIX_VOCAB_PORT`，本应用兼容该名称。 |
| `ZECTRIX_TTS_PYTHON` | 可选。Hub 语音环境的 Python。为空则朗读报「未安装」，阅读不受影响。 |

所有请求都带 `Authorization: Bearer <ZECTRIX_APP_TOKEN>`，且禁止重定向。

### `GET /runtime/status`

```json
{ "active": true, "lease": 3, "device": { "connected": true } }
```

`active == false` 表示用户切到了别的应用。此时设备按键和网页上的设备操作会被拒绝，但刷新简报仍然可以做。`lease` 变化会停下正在进行的朗读。

### `GET /runtime/events?after=<cursor>`

```json
{
  "cursor": 4,
  "events": [
    { "lease": 3, "event": { "type": "key", "key": "Down", "action": "Short" } }
  ]
}
```

只处理 `lease` 与当前租约一致的事件。本应用识别的 `type`：

- `key`：`key` 为 `Up`、`Down`、`Confirm`、`Back`、`Left`、`Cancel`、`Return`。
- `disconnected`：停止朗读。

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
        "content": "AI 日报",
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

响应体是 PNG。`present: false` 只生成预览、不推到屏幕（词汇应用在未激活时这样调用）。本应用假定 `y` 是文字的垂直中心，并在发送前自行折行，不依赖 Hub 自动换行。外框 `rect` 按描边使用，不用它做实心反色，因为词汇应用里唯一的 `rect` 是屏幕边框。

列表每页 5 条（标题 + 来源）。详情页是标题、来源、时间和分页摘要。

### `POST /runtime/audio`

- 请求体：16 kHz、单声道、16-bit PCM WAV。
- 头：`Content-Type: audio/wav`，`X-Lease`，`X-Volume`（0–100）。
- 成功时 JSON 里 `result` 必须是 `Ok(Completed)`。本应用据此认为播放被 Hub 阻塞到结束，连播才能一条接一条。

### `POST /runtime/stop`

```json
{ "lease": 3 }
```

连播被任意键打断时调用，用来截断当前这一条。

网页自身的防护与词汇应用相同：校验 `Host` 为 `127.0.0.1:<端口>` 或 `localhost:<端口>`，校验 `Origin`，非 GET 还要带 `X-App-Token`（这是每次启动生成的页面令牌，不是 Hub 令牌）。响应带 `nosniff`、`no-store` 和较严的 `Content-Security-Policy`。

## 可替换的本地模型

```rust
#[async_trait]
pub trait LlmBackend: Send + Sync {
    fn id(&self) -> &str;
    fn model_name(&self) -> &str;
    async fn complete(&self, request: &LlmRequest) -> Result<String>;
}
```

`OllamaBackend` 把聚类压成提示词，请求 `POST {ollama_url}/api/chat`，`stream: false`，`think: false`，`format: json`。地址只允许本机 `http://127.0.0.1` 或 `http://localhost`，避免设置页变成开放代理。

模型应返回：

```json
{
  "items": [
    { "id": "<聚类 id>", "title": "", "summary": "", "tts": "", "importance": 80 }
  ]
}
```

思考标签和 Markdown 代码块会被剥掉。解析失败、超时或连接被拒绝时，`compose` 改用信源摘要，并在简报的 `warnings` 里写明原因。换一种运行时（llama.cpp、MLX，或你信任的另一台机器）时，实现这个 trait，并在 `pipeline::backend_for` 里返回它即可。

没有模型时，语言切换只能改界面文案和讲稿的开头（「来自某来源」/ `From …`），摘要保持信源原文。这是故意的：没有模型就不假装能翻译。

## 应用数据

全部在 `ZECTRIX_APP_DATA` 下，不写回安装包：

```
settings.json          语言、时间、条数、朗读、模型、信源开关
sources.toml           可选。存在则覆盖安装目录里的默认证源
digests/YYYY-MM-DD.json
audio/<sha256>.wav     讲稿缓存
logs/tts.log
```

`digests` 里除了展示用的条目，还保存聚类原文，这样断网也能换语言重写。日期参数只接受 `YYYY-MM-DD`，避免路径穿越。

## 默认证源

2026-10-14 实际请求过。失效的没有放进 `sources.toml`。

| 信源 | 地址 | 结果 |
| --- | --- | --- |
| OpenAI | `https://openai.com/news/rss.xml` | RSS，可用 |
| Google DeepMind | `https://deepmind.google/blog/rss.xml` | RSS，可用 |
| Google AI Blog | `https://blog.google/innovation-and-ai/technology/ai/rss/` | RSS，可用 |
| MIT Technology Review | `https://www.technologyreview.com/topic/artificial-intelligence/feed/` | RSS，可用 |
| The Verge AI | `https://www.theverge.com/rss/ai-artificial-intelligence/index.xml` | Atom，可用 |
| Hugging Face | `https://huggingface.co/blog/feed.xml` | RSS，可用 |
| TechCrunch AI | `https://techcrunch.com/category/artificial-intelligence/feed/` | RSS，可用 |
| 量子位 | `https://www.qbitai.com/feed` | RSS，可用 |
| 机器之心 | `https://raw.githubusercontent.com/osnsyc/Wechat-Scholar/main/channels/gh_dbc0a5474692.xml` | 可用的公开镜像。官网 `https://www.jiqizhixin.com/rss` 返回 HTML，已弃用 |
| Hacker News | `https://hacker-news.firebaseio.com/v0` | 官方 API。只保留标题像 AI 新闻、且落在时间窗内的故事 |
| arXiv | `https://export.arxiv.org/api/query` | `cs.AI` 与 `cs.CL`，按提交时间，条数有上限 |

已放弃：

- Anthropic：`/rss.xml`、`/news/rss.xml` 等为 404，没有找到公开 RSS/Atom。相关新闻仍可能从 The Verge、TechCrunch 或 HN 进来。
- Meta AI：`https://ai.meta.com/blog/rss/` 为 404。

时间窗默认约 30 小时（arXiv 48 小时，因为按天发布），避免早晨的简报漏掉前一晚的稿件。

## 开发、测试与打包

需要 Rust stable（1.89 或更新）和 Python 3 标准库。设备和字体渲染在 Hub 里完成。

```sh
cargo test --locked
cargo build --release --locked
./scripts/package-app.sh          # 仅 macOS Apple Silicon 会打出可安装包
python3 scripts/verify-package.py dist/*.zip
```

在 Linux 或其他架构上检查 ZIP 契约（这个包不能拿去装 Hub）：

```sh
ZECTRIX_ALLOW_HOST_PACKAGE=1 ./scripts/package-app.sh
```

不连 Hub 时打开设置页：

```sh
ZECTRIX_DEV=1 cargo run
# http://127.0.0.1:43117
```

开发模式使用 `./dev-data`，不连接 Hub，画面预览由网页根据同一套视图模型画出。

抓取新闻支持环境变量中的 HTTP 和 SOCKS5/SOCKS5h 代理。本机 Hub 与 Ollama 请求始终绕过代理。

测试覆盖：RSS/Atom/arXiv 解析、HTML 冒充 feed、HN 过滤、URL 去重、标题聚类、启发式排序、模型 JSON（代码块与 `<think>`）、模型不可用时的回退摘要、设置校验、按日期归档，以及一个假 Hub 上的集成测试（刷新、租约隔离、上/下/确认/返回）。

CI 在 `.github/workflows/ci.yml`：Ubuntu 上 `cargo test --locked`，macOS 15 Apple Silicon 上再打包并上传 ZIP。不下载模型，不访问真机，不自动发布 Release。

## 在真机上还需要核对的事

编写时读到了公开的词汇应用，**没有读到私有的 `itopinion/zectrix-hub`**（本环境没有可用的 GitHub 凭据）。因此下面几项是假设，需要在 Apple Silicon Mac 和一台 Note 4 上确认：

- 端口名称已核对 Hub v1 源码并通过模拟 Hub 启动测试；完整安装流程仍需实测。
- `/runtime/*` 的字段是否与词汇应用完全一致，尤其是音频 `result` 字符串和 Short 按键事件。
- 卡片 JSON 的 `y` 是垂直中心、基线，还是顶边。现在按词汇应用的坐标风格，把 `y` 当作中心。
- Hub 自带的 Kokoro 环境能否加载中文 `lang_code=z`、声音 `zf_xiaobei`。英文默认 `af_heart`，与词汇应用相同。
- `POST /runtime/stop` 能否在 `POST /runtime/audio` 还没返回时打断播放。
- 墨水屏全帧刷新是否闪得可以接受。本应用每次状态变化都提交整页。
- 用 Hub 安装 `macos-aarch64` ZIP 的完整路径，包括语音组件未安装时的提示。

## 许可

程序代码使用 MIT 许可，见 `LICENSE`。第三方组件和模型各自遵循原许可，见 `THIRD_PARTY_NOTICES.md`。

## Codex 整理与三键操作（0.1.3）

网页「摘要来源」可选 Codex、信源原文或 Ollama。Codex 调用已登录的本机 CLI，以只读、临时会话和 JSON Schema 输出整理新闻；新闻原文作为不可信数据传入，不需要安装本地文本模型。使用既有 Codex 账号额度。失败时保留可读的信源摘要并显示警告。

在应用数据目录建立 `local-runtime.json` 指定本机 CLI 的绝对路径，例如 `{"codex_bin":"/absolute/path/to/codex"}`；可选 `codex_model` 指定模型。不把该机器配置或账号凭证放进安装包。

Note 4 只有上、下、确认三个键：列表短按确认进入，详情短按确认返回；开启朗读后进入详情自动朗读；重新进入可重听，连播使用网页按钮。选中行使用黑底白字。语音合成在后台进行，返回操作会取消待播放内容。

### 本机 Qwen-TTS

在应用数据目录的 `local-runtime.json` 中设置 `tts_backend: "qwen"`、
`tts_python`（已有 MLX Audio 环境的 Python 路径）和 `qwen_model`（已有
Qwen3-TTS CustomVoice 模型目录），然后重新启动此应用。模型不会打包或自动下载。
Qwen-ASR 是识别模型，不能用于朗读。中文默认 Vivian，英文默认 Ryan，输出
16 kHz 单声道 PCM WAV。语速使用重采样，会同时影响音高。
未配置模型时，页面会显示缺少本机模型；阅读与 Codex 摘要仍可正常使用。
