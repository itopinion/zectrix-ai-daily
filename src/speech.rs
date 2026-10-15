//! Offline local speech workers (Qwen-TTS or Hub Kokoro).

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

struct Worker {
    child: Child,
    input: ChildStdin,
    output: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
}

pub struct Speech {
    worker: Mutex<Option<Worker>>,
    python: PathBuf,
    script: PathBuf,
    log: PathBuf,
    model: Option<PathBuf>,
    qwen: bool,
}

impl Speech {
    pub fn new(app_root: &Path, data_root: &Path) -> Self {
        let local = crate::local::LocalRuntime::load(data_root).unwrap_or_default();
        let qwen = local.tts_backend.as_deref() == Some("qwen") || local.qwen_model.is_some();
        let python = local.tts_python.or_else(|| std::env::var("ZECTRIX_TTS_PYTHON").ok().map(PathBuf::from)).unwrap_or_default();
        Self {
            worker: Mutex::new(None),
            python,
            script: app_root.join(if qwen { "workers/qwen_tts.py" } else { "workers/tts.py" }),
            model: local.qwen_model,
            qwen,
            log: data_root.join("logs/tts.log"),
        }
    }

    pub fn installed(&self) -> bool {
        self.python.is_file() && (!self.qwen || self.model.as_ref().is_some_and(|p| p.join("config.json").is_file()))
    }

    pub fn backend(&self) -> &'static str {
        if self.qwen { "qwen" } else { "kokoro" }
    }

    pub fn cache_identity(&self) -> String {
        format!("{}|{}", self.backend(), self.model.as_ref().map(|p| p.to_string_lossy()).unwrap_or_default())
    }

    async fn spawn(&self) -> Result<Worker> {
        if !self.installed() {
            if self.qwen { bail!("缺少本机 Qwen-TTS 模型，请在 local-runtime.json 配置 qwen_model；Qwen-ASR 不能生成语音。"); }
            bail!("语音组件未安装。请在 ZECTRIX Hub 安装本地语音环境；简报仍可阅读。");
        }
        if let Some(parent) = self.log.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let log = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log)?;
        let mut command = Command::new(&self.python);
        if let Some(model) = &self.model { command.env("ZECTRIX_QWEN_MODEL", model); }
        let mut child = command
            .arg("-u")
            .arg(&self.script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .kill_on_drop(true)
            .spawn()
            .context("无法启动本地语音进程")?;
        let input = child.stdin.take().context("语音进程没有标准输入")?;
        let stdout = child.stdout.take().context("语音进程没有标准输出")?;
        let mut output = BufReader::new(stdout).lines();
        let line = tokio::time::timeout(Duration::from_secs(120), output.next_line())
            .await
            .context("语音模型启动超时")??
            .context("语音模型没有就绪信号")?;
        let ready: Value = serde_json::from_str(&line)?;
        if ready["ready"] != true {
            bail!("语音模型没有就绪：{line}");
        }
        Ok(Worker {
            child,
            input,
            output,
        })
    }

    pub async fn call(&self, request: Value) -> Result<Value> {
        let mut guard = self.worker.lock().await;
        if guard.is_none() {
            *guard = Some(self.spawn().await?);
        }
        let worker = guard.as_mut().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(if self.qwen { 300 } else { 90 }), async {
            worker
                .input
                .write_all(format!("{request}\n").as_bytes())
                .await?;
            worker.input.flush().await?;
            let line = worker.output.next_line().await?.context("语音进程已退出")?;
            let value: Value = serde_json::from_str(&line)?;
            if value["ok"] != true {
                bail!("{}", value["error"].as_str().unwrap_or("语音合成失败"));
            }
            Ok(value)
        })
        .await;
        match result {
            Ok(Ok(value)) => Ok(value),
            other => {
                if let Some(mut worker) = guard.take() {
                    let _ = worker.child.kill().await;
                }
                match other {
                    Ok(Err(error)) => Err(error),
                    _ => bail!("本地语音超时"),
                }
            }
        }
    }
}
