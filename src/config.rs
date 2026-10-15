//! Settings and the source catalog. `sources.toml` is the shipped list.
//! A copy at `$ZECTRIX_APP_DATA/sources.toml` wins, so an installed app can
//! change feeds without a rebuild. Enablement overrides live in settings.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourceDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_zh: String,
    /// `feed`, `hn`, or `arxiv`.
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default = "default_weight")]
    pub weight: f32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_window")]
    pub window_hours: u32,
    #[serde(default = "default_max_items")]
    pub max_items: usize,
}

impl SourceDef {
    pub fn label<'a>(&'a self, language: &str) -> &'a str {
        if language == "zh" && !self.name_zh.is_empty() {
            &self.name_zh
        } else {
            &self.name
        }
    }
}

fn default_weight() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_window() -> u32 {
    30
}
fn default_max_items() -> usize {
    12
}

#[derive(Clone, Debug, Deserialize)]
struct Catalog {
    source: Vec<SourceDef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    #[serde(default = "default_backend")]
    pub llm_backend: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_hour")]
    pub refresh_hour: u8,
    #[serde(default)]
    pub refresh_minute: u8,
    #[serde(default = "default_count")]
    pub item_count: u8,
    #[serde(default = "default_true")]
    pub tts_enabled: bool,
    #[serde(default = "default_speed")]
    pub tts_speed: f32,
    #[serde(default = "default_volume")]
    pub volume: u8,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default = "default_model")]
    pub ollama_model: String,
    #[serde(default = "default_voice_en")]
    pub voice_en: String,
    #[serde(default = "default_voice_zh")]
    pub voice_zh: String,
    /// Source id → enabled. Missing ids use the catalog default.
    #[serde(default)]
    pub source_overrides: BTreeMap<String, bool>,
    /// Empty means "today" on the device.
    #[serde(default)]
    pub device_date: String,
}

fn default_language() -> String {
    "zh".into()
}
fn default_backend() -> String { "ollama".into() }
fn default_hour() -> u8 {
    7
}
fn default_count() -> u8 {
    12
}
fn default_speed() -> f32 {
    1.0
}
fn default_volume() -> u8 {
    70
}
fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".into()
}
fn default_model() -> String {
    "qwen3".into()
}
fn default_voice_en() -> String {
    "af_heart".into()
}
fn default_voice_zh() -> String {
    "zf_xiaobei".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            llm_backend: default_backend(),
            language: default_language(),
            refresh_hour: default_hour(),
            refresh_minute: 0,
            item_count: default_count(),
            tts_enabled: true,
            tts_speed: default_speed(),
            volume: default_volume(),
            ollama_url: default_ollama_url(),
            ollama_model: default_model(),
            voice_en: default_voice_en(),
            voice_zh: default_voice_zh(),
            source_overrides: BTreeMap::new(),
            device_date: String::new(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.llm_backend.as_str(), "codex" | "ollama" | "fallback") {
            bail!("摘要来源须为 codex、ollama 或 fallback");
        }
        if self.language != "zh" && self.language != "en" {
            bail!("language 只能是 zh 或 en");
        }
        if self.refresh_hour > 23 || self.refresh_minute > 59 {
            bail!("刷新时间无效");
        }
        if !(5..=20).contains(&self.item_count) {
            bail!("条数须为 5–20");
        }
        if !(0.7..=1.4).contains(&self.tts_speed) {
            bail!("语速须为 0.7–1.4");
        }
        if self.volume > 100 {
            bail!("音量须为 0–100");
        }
        validate_loopback_url(&self.ollama_url)?;
        validate_voice(&self.voice_en)?;
        validate_voice(&self.voice_zh)?;
        if !self.device_date.is_empty() && !is_date(&self.device_date) {
            bail!("设备日期须为 YYYY-MM-DD");
        }
        for key in self.source_overrides.keys() {
            if !is_source_id(key) {
                bail!("未知信源编号");
            }
        }
        Ok(())
    }

    pub fn source_enabled(&self, source: &SourceDef) -> bool {
        self.source_overrides
            .get(&source.id)
            .copied()
            .unwrap_or(source.enabled)
    }

    pub fn voice_for(&self, language: &str) -> &str {
        if language == "zh" {
            &self.voice_zh
        } else {
            &self.voice_en
        }
    }

    pub fn load(root: &Path) -> Result<Self> {
        let path = settings_path(root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let settings: Self = serde_json::from_slice(&fs::read(&path)?)
            .with_context(|| format!("无法读取 {}", path.display()))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        fs::create_dir_all(root)?;
        atomic_write(&settings_path(root), &serde_json::to_vec_pretty(self)?)
    }
}

pub fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

fn validate_voice(voice: &str) -> Result<()> {
    if voice.is_empty()
        || voice.len() > 32
        || !voice
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        bail!("声音编号只能是小写字母、数字和下划线");
    }
    Ok(())
}

fn validate_loopback_url(raw: &str) -> Result<()> {
    if raw.trim().is_empty() {
        return Ok(());
    }
    let url = url::Url::parse(raw).context("Ollama 地址无效")?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Ollama 地址只能是本机 http://127.0.0.1 或 http://localhost");
    }
    Ok(())
}

pub fn is_source_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn is_date(value: &str) -> bool {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() && value.len() == 10
}

pub fn load_sources(app_root: &Path, data_root: &Path) -> Result<Vec<SourceDef>> {
    let override_path = data_root.join("sources.toml");
    let bundled = app_root.join("sources.toml");
    let text = if override_path.exists() {
        fs::read_to_string(&override_path)
            .with_context(|| format!("无法读取 {}", override_path.display()))?
    } else if bundled.exists() {
        fs::read_to_string(&bundled).with_context(|| format!("无法读取 {}", bundled.display()))?
    } else {
        include_str!("../sources.toml").to_string()
    };
    parse_sources(&text)
}

pub fn parse_sources(text: &str) -> Result<Vec<SourceDef>> {
    let catalog: Catalog = toml::from_str(text).context("sources.toml 无法解析")?;
    if catalog.source.is_empty() {
        bail!("sources.toml 至少需要一个信源");
    }
    let mut seen = BTreeMap::new();
    for source in &catalog.source {
        if !is_source_id(&source.id) {
            bail!("信源 id 无效：{}", source.id);
        }
        if seen.insert(source.id.clone(), ()).is_some() {
            bail!("信源 id 重复：{}", source.id);
        }
        if !matches!(source.kind.as_str(), "feed" | "hn" | "arxiv") {
            bail!("信源 {} 的 kind 只能是 feed、hn 或 arxiv", source.id);
        }
        if !(0.1..=5.0).contains(&source.weight) {
            bail!("信源 {} 的权重无效", source.id);
        }
        if source.window_hours == 0 || source.window_hours > 168 {
            bail!("信源 {} 的时间窗无效", source.id);
        }
        if source.max_items == 0 || source.max_items > 40 {
            bail!("信源 {} 的条数上限无效", source.id);
        }
        let parsed = url::Url::parse(&source.url)
            .with_context(|| format!("信源 {} 的地址无效", source.id))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            bail!("信源 {} 只允许 http 或 https", source.id);
        }
    }
    Ok(catalog.source)
}

pub fn local_today(now: DateTime<Local>) -> String {
    now.format("%Y-%m-%d").to_string()
}

/// Next local instant strictly after `now` at hour:minute.
pub fn next_run(now: DateTime<Local>, hour: u8, minute: u8) -> DateTime<Local> {
    let today = now.date_naive();
    if let Some(candidate) = at_local(today, hour, minute) {
        if candidate > now {
            return candidate;
        }
    }
    let tomorrow = today.succ_opt().unwrap_or(today);
    at_local(tomorrow, hour, minute).unwrap_or_else(|| now + chrono::Duration::hours(24))
}

fn at_local(date: NaiveDate, hour: u8, minute: u8) -> Option<DateTime<Local>> {
    let naive = date.and_hms_opt(hour.into(), minute.into(), 0)?;
    naive
        .and_local_timezone(Local)
        .single()
        .or_else(|| naive.and_local_timezone(Local).earliest())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("pending");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn bundled_catalog_parses() {
        let sources = parse_sources(include_str!("../sources.toml")).unwrap();
        assert!(sources
            .iter()
            .any(|source| source.id == "openai" && source.kind == "feed"));
        assert!(sources.iter().any(|source| source.kind == "hn"));
        assert!(sources.iter().any(|source| source.kind == "arxiv"));
        assert!(sources.iter().all(|source| source.kind != "html"));
    }

    #[test]
    fn settings_roundtrip_and_rejection() {
        let root = std::env::temp_dir().join(format!("ai-daily-settings-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(Settings::load(&root).unwrap().refresh_hour, 7);
        let mut settings = Settings::default();
        settings.item_count = 15;
        settings.language = "en".into();
        settings.save(&root).unwrap();
        assert_eq!(Settings::load(&root).unwrap().item_count, 15);
        settings.volume = 140;
        assert!(settings.save(&root).is_err());
        settings.volume = 40;
        settings.ollama_url = "http://example.com:11434".into();
        assert!(settings.validate().is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn next_run_rolls_to_tomorrow_after_the_slot() {
        let morning = Local
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 10, 14)
                    .unwrap()
                    .and_hms_opt(6, 0, 0)
                    .unwrap(),
            )
            .single()
            .unwrap();
        let next = next_run(morning, 7, 0);
        assert_eq!(
            next.format("%Y-%m-%d %H:%M").to_string(),
            "2026-10-14 07:00"
        );
        let evening = Local
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 10, 14)
                    .unwrap()
                    .and_hms_opt(8, 0, 0)
                    .unwrap(),
            )
            .single()
            .unwrap();
        let next = next_run(evening, 7, 30);
        assert_eq!(
            next.format("%Y-%m-%d %H:%M").to_string(),
            "2026-10-15 07:30"
        );
    }
}
