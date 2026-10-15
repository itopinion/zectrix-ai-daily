//! Machine-specific executable/model paths. Stored in app data, never in a package.
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct LocalRuntime {
    pub codex_bin: Option<PathBuf>,
    pub codex_model: Option<String>,
    pub tts_python: Option<PathBuf>,
    pub tts_backend: Option<String>,
    pub qwen_model: Option<PathBuf>,
}

impl LocalRuntime {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("local-runtime.json");
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("local-runtime.json 格式错误"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
}
