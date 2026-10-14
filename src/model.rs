//! Shared records. Digests are the archived product; clusters are the raw
//! material kept so a language switch can regenerate without refetching.

use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Article {
    pub id: String,
    pub source_id: String,
    pub source_name: String,
    pub title: String,
    pub url: String,
    pub summary: String,
    /// RFC3339 UTC. `None` means the feed did not give a usable time.
    pub published_at: Option<String>,
    pub popularity: u32,
    pub weight: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Cluster {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub url: String,
    pub source_names: Vec<String>,
    pub published_at: Option<String>,
    pub popularity: u32,
    pub weight: f32,
    /// Heuristic score filled in by [`crate::rank`]. Higher is more important.
    pub heuristic: f32,
    pub articles: Vec<Article>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DigestItem {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub tts_script: String,
    pub source: String,
    pub sources: Vec<String>,
    pub url: String,
    pub published_at: Option<String>,
    pub score: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Digest {
    pub schema: u32,
    /// Local calendar date, `YYYY-MM-DD`, of the briefing this file belongs to.
    pub date: String,
    /// `zh` or `en`. Controls generated prose, not the original feed language.
    pub language: String,
    pub generated_at: String,
    /// `ollama` or `fallback`.
    pub backend: String,
    pub model: String,
    pub warnings: Vec<String>,
    pub items: Vec<DigestItem>,
    /// Ranked clusters used to build `items`. Kept so a language switch can
    /// regenerate from disk while the network is down.
    pub clusters: Vec<Cluster>,
}

impl Digest {
    pub fn empty(date: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA,
            date: date.into(),
            language: language.into(),
            generated_at: String::new(),
            backend: "fallback".into(),
            model: String::new(),
            warnings: Vec::new(),
            items: Vec::new(),
            clusters: Vec::new(),
        }
    }
}

/// What the settings page needs. Clusters stay on disk.
#[derive(Clone, Debug, Serialize)]
pub struct PublicDigest {
    pub date: String,
    pub language: String,
    pub generated_at: String,
    pub backend: String,
    pub model: String,
    pub warnings: Vec<String>,
    pub items: Vec<DigestItem>,
}

impl Digest {
    pub fn public_view(&self) -> PublicDigest {
        PublicDigest {
            date: self.date.clone(),
            language: self.language.clone(),
            generated_at: self.generated_at.clone(),
            backend: self.backend.clone(),
            model: self.model.clone(),
            warnings: self.warnings.clone(),
            items: self.items.clone(),
        }
    }
}
