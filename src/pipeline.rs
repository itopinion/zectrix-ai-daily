//! Fetch → cluster → rank → compose. The scheduler and the refresh button
//! both call [`run`]. Language changes call [`regenerate`], which reuses the
//! clusters already stored for that date.

use crate::cluster::cluster_articles;
use crate::config::Settings;
use crate::fetch::{self, client as http_client};
use crate::llm::{self, FallbackOnly, LlmBackend, OllamaBackend};
use crate::model::Digest;
use crate::rank::rank;
use crate::store;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

pub fn prepare(
    articles: Vec<crate::model::Article>,
    now: DateTime<Utc>,
) -> Vec<crate::model::Cluster> {
    rank(cluster_articles(articles), now)
}

pub fn backend_for(settings: &Settings, root: &Path) -> Result<Box<dyn LlmBackend>> {
    if settings.llm_backend == "codex" {
        return Ok(Box::new(llm::CodexBackend::new(root)?));
    }
    if settings.llm_backend == "fallback" { return Ok(Box::new(FallbackOnly)); }
    if settings.ollama_url.trim().is_empty() || settings.ollama_model.trim().is_empty() {
        return Ok(Box::new(FallbackOnly));
    }
    Ok(Box::new(OllamaBackend::new(
        &settings.ollama_url,
        &settings.ollama_model,
    )?))
}

pub async fn build_digest(
    articles: Vec<crate::model::Article>,
    settings: &Settings,
    date: &str,
    warnings: Vec<String>,
    llm: &dyn LlmBackend,
) -> Digest {
    let now = Utc::now();
    let clusters = prepare(articles, now);
    let composed = llm::compose(
        &clusters,
        &settings.language,
        settings.item_count as usize,
        llm,
        warnings,
    )
    .await;
    Digest {
        schema: crate::model::SCHEMA,
        date: date.to_string(),
        language: settings.language.clone(),
        generated_at: Utc::now().to_rfc3339(),
        backend: composed.backend,
        model: composed.model,
        warnings: composed.warnings,
        items: composed.items,
        clusters,
    }
}

pub async fn run(
    data_root: &Path,
    app_root: &Path,
    settings: &Settings,
    date: &str,
) -> Result<Digest> {
    let sources = crate::config::load_sources(app_root, data_root)?;
    let enabled: Vec<_> = sources
        .into_iter()
        .filter(|source| settings.source_enabled(source))
        .collect();
    if enabled.is_empty() {
        bail!("没有启用的信源");
    }
    let http = http_client()?;
    let (articles, warnings) = fetch::fetch_all(&http, &enabled).await;
    if articles.is_empty() {
        bail!(
            "没有抓到时间窗内的条目{}",
            if warnings.is_empty() {
                String::new()
            } else {
                format!("（{}）", warnings.join("；"))
            }
        );
    }
    let llm = backend_for(settings, data_root)?;
    let digest = build_digest(articles, settings, date, warnings, llm.as_ref()).await;
    store::save_digest(data_root, &digest)?;
    Ok(digest)
}

pub async fn regenerate(data_root: &Path, settings: &Settings, date: &str) -> Result<Digest> {
    let existing = store::load_digest(data_root, date)?
        .ok_or_else(|| anyhow::anyhow!("这一天还没有简报，请先刷新"))?;
    if existing.clusters.is_empty() {
        bail!("这一天的归档没有原始聚类，无法换语言重写");
    }
    let llm = backend_for(settings, data_root)?;
    let articles: Vec<_> = existing
        .clusters
        .iter()
        .flat_map(|cluster| cluster.articles.clone())
        .collect();
    // Rebuild clusters so a changed ranking function still applies, but do
    // not touch the network.
    let digest = build_digest(articles, settings, date, Vec::new(), llm.as_ref()).await;
    store::save_digest(data_root, &digest)?;
    Ok(digest)
}

/// Shared handle used by tests that want the trait object without the server.
pub type SharedBackend = Arc<dyn LlmBackend>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Article;
    fn article(title: &str, source: &str, weight: f32, minutes_ago: i64) -> Article {
        let published = (Utc::now() - chrono::Duration::minutes(minutes_ago)).to_rfc3339();
        Article {
            id: format!("{source}-{title}"),
            source_id: source.into(),
            source_name: source.into(),
            title: title.into(),
            url: format!("https://example.com/{source}/{}", title.replace(' ', "-")),
            summary: format!("{title}. Detail two. Detail three. Detail four."),
            published_at: Some(published),
            popularity: if source == "Hacker News" { 80 } else { 0 },
            weight,
        }
    }

    #[tokio::test]
    async fn fallback_path_ranks_and_caps_without_a_model() {
        let articles = vec![
            article("OpenAI releases a pocket model", "OpenAI", 1.4, 30),
            article(
                "OpenAI releases a pocket model for phones",
                "The Verge",
                1.0,
                40,
            ),
            article("DeepMind predicts the weather", "DeepMind", 1.2, 50),
            article("A quiet chip note from a blog", "Blog", 0.3, 10),
        ];
        let mut settings = Settings::default();
        settings.language = "en".into();
        settings.item_count = 5;
        settings.ollama_url.clear();
        let digest =
            build_digest(articles, &settings, "2026-10-14", Vec::new(), &FallbackOnly).await;
        assert_eq!(digest.backend, "fallback");
        assert_eq!(
            digest.items.len(),
            3,
            "the two OpenAI headlines must cluster"
        );
        assert_eq!(digest.items[0].sources.len(), 2);
        assert!(digest.items[0].summary.matches('.').count() >= 2);
        assert!(digest
            .items
            .iter()
            .any(|item| item.title.contains("DeepMind")));
    }
}
