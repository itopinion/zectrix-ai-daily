//! JSON archives under `ZECTRIX_APP_DATA`. One file per local date.

use crate::config::{atomic_write, is_date};
use crate::model::Digest;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn digest_path(root: &Path, date: &str) -> PathBuf {
    root.join("digests").join(format!("{date}.json"))
}

pub fn save_digest(root: &Path, digest: &Digest) -> Result<()> {
    if !is_date(&digest.date) {
        bail!("归档日期无效");
    }
    let bytes = serde_json::to_vec_pretty(digest)?;
    atomic_write(&digest_path(root, &digest.date), &bytes)
}

pub fn load_digest(root: &Path, date: &str) -> Result<Option<Digest>> {
    if !is_date(date) {
        bail!("归档日期无效");
    }
    let path = digest_path(root, date);
    if !path.exists() {
        return Ok(None);
    }
    let digest = serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("简报文件损坏：{}", path.display()))?;
    Ok(Some(digest))
}

pub fn list_dates(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("digests");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut dates = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(date) = name.strip_suffix(".json") {
            if is_date(date) {
                dates.push(date.to_string());
            }
        }
    }
    dates.sort();
    dates.reverse();
    Ok(dates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Digest;

    #[test]
    fn archives_roundtrip_per_date() {
        let root = std::env::temp_dir().join(format!("ai-daily-store-{}", uuid::Uuid::new_v4()));
        let mut digest = Digest::empty("2026-10-14", "zh");
        digest.generated_at = "2026-10-14T07:00:00Z".into();
        digest.backend = "fallback".into();
        save_digest(&root, &digest).unwrap();
        let loaded = load_digest(&root, "2026-10-14").unwrap().unwrap();
        assert_eq!(loaded.language, "zh");
        assert_eq!(list_dates(&root).unwrap(), vec!["2026-10-14".to_string()]);
        assert!(load_digest(&root, "../secret").is_err());
        let _ = fs::remove_dir_all(root);
    }
}
