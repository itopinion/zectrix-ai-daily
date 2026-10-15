//! Network fetchers. One failing source becomes a warning; it does not drop
//! the rest of the briefing. Hacker News uses the official Firebase API and
//! keeps only stories whose titles look like AI news.

use crate::config::SourceDef;
use crate::model::Article;
use crate::parse::{parse_feed, FeedItem};
use crate::text::{canonical_url, collapse_ws, stable_id};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::sync::Semaphore;

const USER_AGENT: &str = "zectrix-ai-daily/0.1 (local ZECTRIX Hub briefing)";

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(StdDuration::from_secs(25))
        .connect_timeout(StdDuration::from_secs(8))
        .gzip(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?)
}

pub async fn fetch_all(
    client: &reqwest::Client,
    sources: &[SourceDef],
) -> (Vec<Article>, Vec<String>) {
    let now = Utc::now();
    let mut set = tokio::task::JoinSet::new();
    for source in sources {
        let source = source.clone();
        let client = client.clone();
        set.spawn(async move {
            let id = source.id.clone();
            match fetch_source(&client, &source, now).await {
                Ok(articles) => (id, Ok(articles)),
                Err(error) => (id, Err(format!("{error:#}"))),
            }
        });
    }
    let mut articles = Vec::new();
    let mut warnings = Vec::new();
    while let Some(joined) = set.join_next().await {
        let Ok((id, result)) = joined else {
            warnings.push("a source task panicked".into());
            continue;
        };
        match result {
            Ok(mut batch) => articles.append(&mut batch),
            Err(error) => warnings.push(format!("{id}: {error}")),
        }
    }
    (articles, warnings)
}

pub async fn fetch_source(
    client: &reqwest::Client,
    source: &SourceDef,
    now: DateTime<Utc>,
) -> Result<Vec<Article>> {
    match source.kind.as_str() {
        "feed" => fetch_feed(client, source, now).await,
        "hn" => fetch_hn(client, source, now).await,
        "arxiv" => fetch_arxiv(client, source, now).await,
        other => bail!("unsupported source kind {other}"),
    }
}

async fn fetch_feed(
    client: &reqwest::Client,
    source: &SourceDef,
    now: DateTime<Utc>,
) -> Result<Vec<Article>> {
    let xml = get_text(client, &source.url).await?;
    let items = parse_feed(&xml).with_context(|| format!("无法解析 {}", source.id))?;
    Ok(limit_items(
        map_feed_items(source, &items, now),
        source.max_items,
    ))
}

async fn fetch_arxiv(
    client: &reqwest::Client,
    source: &SourceDef,
    now: DateTime<Utc>,
) -> Result<Vec<Article>> {
    let categories = if source.categories.is_empty() {
        vec!["cs.AI".to_string(), "cs.CL".to_string()]
    } else {
        source.categories.clone()
    };
    let query = categories
        .iter()
        .map(|category| format!("cat:{category}"))
        .collect::<Vec<_>>()
        .join("+OR+");
    let max_results = source.max_items.clamp(1, 20);
    let url = format!(
        "{}?search_query=({})&sortBy=submittedDate&sortOrder=descending&start=0&max_results={max_results}",
        source.url.trim_end_matches('/'),
        query
    );
    let xml = get_text(client, &url).await?;
    let items = parse_feed(&xml).context("无法解析 arXiv Atom")?;
    Ok(limit_items(
        map_feed_items(source, &items, now),
        source.max_items,
    ))
}

fn map_feed_items(source: &SourceDef, items: &[FeedItem], now: DateTime<Utc>) -> Vec<Article> {
    let mut articles = Vec::new();
    for item in items {
        let published = parse_stamp(&item.published_raw);
        if !within_window(published, now, source.window_hours) {
            continue;
        }
        let url = if item.url.is_empty() {
            continue;
        } else {
            item.url.clone()
        };
        articles.push(make_article(
            source,
            &item.title,
            &url,
            &item.summary,
            published,
            0,
        ));
    }
    articles.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    articles
}

async fn fetch_hn(
    client: &reqwest::Client,
    source: &SourceDef,
    now: DateTime<Utc>,
) -> Result<Vec<Article>> {
    let base = source.url.trim_end_matches('/');
    let top = get_ids(client, &format!("{base}/topstories.json"))
        .await
        .unwrap_or_default();
    let fresh = get_ids(client, &format!("{base}/newstories.json"))
        .await
        .unwrap_or_default();
    let mut ids = Vec::new();
    for id in top.into_iter().take(40).chain(fresh.into_iter().take(20)) {
        if !ids.contains(&id) {
            ids.push(id);
        }
        if ids.len() >= 50 {
            break;
        }
    }
    if ids.is_empty() {
        bail!("Hacker News 没有返回故事编号");
    }
    let semaphore = Arc::new(Semaphore::new(8));
    let mut set = tokio::task::JoinSet::new();
    for id in ids {
        let client = client.clone();
        let semaphore = semaphore.clone();
        let base = base.to_string();
        set.spawn(async move {
            let _permit = semaphore.acquire().await.ok()?;
            let url = format!("{base}/item/{id}.json");
            let value: Value = client.get(url).send().await.ok()?.json().await.ok()?;
            Some(value)
        });
    }
    let mut articles = Vec::new();
    while let Some(joined) = set.join_next().await {
        let Ok(Some(value)) = joined else {
            continue;
        };
        if let Some(article) = hn_article(source, &value, now) {
            articles.push(article);
        }
    }
    articles.sort_by(|a, b| {
        b.popularity
            .cmp(&a.popularity)
            .then(b.published_at.cmp(&a.published_at))
    });
    Ok(limit_items(articles, source.max_items))
}

fn hn_article(source: &SourceDef, value: &Value, now: DateTime<Utc>) -> Option<Article> {
    if value["type"].as_str() != Some("story") || value["dead"] == true || value["deleted"] == true
    {
        return None;
    }
    let title = collapse_ws(value["title"].as_str().unwrap_or(""));
    if title.is_empty() || !ai_related(&title) {
        return None;
    }
    let unix = value["time"].as_i64()?;
    let published = DateTime::<Utc>::from_timestamp(unix, 0)?;
    if !within_window(Some(published), now, source.window_hours) {
        return None;
    }
    let url = value["url"]
        .as_str()
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "https://news.ycombinator.com/item?id={}",
                value["id"].as_i64().unwrap_or(0)
            )
        });
    let points = value["score"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
    let summary = crate::text::strip_html(value["text"].as_str().unwrap_or(""));
    Some(make_article(
        source,
        &title,
        &url,
        &summary,
        Some(published),
        points,
    ))
}

pub fn ai_related(title: &str) -> bool {
    let lower = title.to_lowercase();
    const PHRASES: &[&str] = &[
        "machine learning",
        "large language",
        "neural net",
        "generative ai",
        "foundation model",
        "language model",
        "人工智能",
        "大模型",
        "语言模型",
        "智能体",
        "深度学习",
        "多模态",
        "a.i.",
    ];
    if PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return true;
    }
    const WORDS: &[&str] = &[
        "ai",
        "llm",
        "llms",
        "gpt",
        "openai",
        "anthropic",
        "claude",
        "gemini",
        "deepmind",
        "deepseek",
        "huggingface",
        "chatgpt",
        "qwen",
        "llama",
        "mistral",
        "grok",
        "copilot",
        "midjourney",
        "sora",
        "perplexity",
        "agentic",
    ];
    tokens(&lower)
        .iter()
        .any(|token| WORDS.contains(&token.as_str()))
}

fn tokens(lower: &str) -> Vec<String> {
    lower
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn make_article(
    source: &SourceDef,
    title: &str,
    url: &str,
    summary: &str,
    published: Option<DateTime<Utc>>,
    popularity: u32,
) -> Article {
    let summary: String = summary.chars().take(1200).collect();
    let canonical = canonical_url(url);
    Article {
        id: stable_id(&format!("{}:{}", source.id, canonical)),
        source_id: source.id.clone(),
        source_name: source.name.clone(),
        title: title.to_string(),
        url: url.to_string(),
        summary,
        published_at: published.map(|stamp| stamp.to_rfc3339()),
        popularity,
        weight: source.weight,
    }
}

pub fn within_window(
    published: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    window_hours: u32,
) -> bool {
    let Some(published) = published else {
        return false;
    };
    if published > now + Duration::hours(6) {
        return false;
    }
    published >= now - Duration::hours(window_hours as i64)
}

pub fn parse_stamp(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.with_timezone(&Utc));
    }
    if let Ok(parsed) = DateTime::parse_from_rfc2822(raw) {
        return Some(parsed.with_timezone(&Utc));
    }
    None
}

fn limit_items(mut articles: Vec<Article>, max_items: usize) -> Vec<Article> {
    articles.truncate(max_items);
    articles
}

async fn get_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let response = client
        .get(url)
        .header(
            "Accept",
            "application/rss+xml, application/atom+xml, application/xml, text/xml, */*",
        )
        .send()
        .await
        .with_context(|| format!("请求失败 {url}"))?;
    if !response.status().is_success() {
        bail!("{url} 返回 {}", response.status());
    }
    let bytes = response.bytes().await?;
    if bytes.len() > 3_000_000 {
        bail!("{url} 超过 3MB，已跳过");
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn get_ids(client: &reqwest::Client, url: &str) -> Result<Vec<u64>> {
    let value: Value = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(value
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| item.as_u64())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hn_filter_rejects_unrelated_stories() {
        assert!(ai_related("Show HN: a local LLM for notes"));
        assert!(ai_related("OpenAI and Anthropic both shipped today"));
        assert!(ai_related("深度求索更新了大模型"));
        assert!(!ai_related("A city replaces its email servers"));
        assert!(!ai_related("The available trail is closed"));
    }

    #[test]
    fn window_drops_old_and_undated_items() {
        let now = DateTime::parse_from_rfc3339("2026-10-14T07:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let fresh = DateTime::parse_from_rfc3339("2026-10-14T05:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let old = DateTime::parse_from_rfc3339("2026-10-12T05:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(within_window(Some(fresh), now, 30));
        assert!(!within_window(Some(old), now, 30));
        assert!(!within_window(None, now, 30));
        assert_eq!(
            parse_stamp("Tue, 13 Oct 2026 11:05:39 +0000")
                .unwrap()
                .format("%Y-%m-%d")
                .to_string(),
            "2026-10-13"
        );
    }

    #[test]
    fn hn_story_json_becomes_an_article() {
        let source = SourceDef {
            id: "hacker-news".into(),
            name: "Hacker News".into(),
            name_zh: String::new(),
            kind: "hn".into(),
            url: "https://hacker-news.firebaseio.com/v0".into(),
            categories: Vec::new(),
            weight: 1.0,
            enabled: true,
            window_hours: 48,
            max_items: 10,
        };
        let now = DateTime::parse_from_rfc3339("2026-10-14T07:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let value = serde_json::json!({
            "id": 42,
            "type": "story",
            "title": "A small local LLM you can run at home",
            "url": "https://example.com/llm?utm_source=hn",
            "score": 120,
            "text": "<p>A pocket model that runs offline.</p>",
            "time": now.timestamp() - 3600
        });
        let article = hn_article(&source, &value, now).unwrap();
        assert_eq!(article.popularity, 120);
        assert!(article.url.contains("example.com/llm"));
        assert_eq!(article.summary, "A pocket model that runs offline.");
        let noise = serde_json::json!({
            "id": 7,
            "type": "story",
            "title": "City council approves a park",
            "time": now.timestamp()
        });
        assert!(hn_article(&source, &noise, now).is_none());
    }
}
