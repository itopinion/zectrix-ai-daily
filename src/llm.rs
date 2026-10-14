//! Pluggable local model. [`LlmBackend`] is the only thing a different runtime
//! (llama.cpp, MLX, a remote box you trust) needs to implement. Ollama is the
//! default. Any error, timeout, or unreadable reply falls through to the
//! extractive briefing in [`compose`].

use crate::model::{Cluster, DigestItem};
use crate::text::first_sentences;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ClusterBrief {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub sources: String,
    pub popularity: u32,
    pub heuristic: f32,
}

#[derive(Clone, Debug)]
pub struct LlmRequest {
    pub language: String,
    pub limit: usize,
    pub clusters: Vec<ClusterBrief>,
}

#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// Stable id stored on the digest, for example `ollama`.
    fn id(&self) -> &str;
    fn model_name(&self) -> &str;
    async fn complete(&self, request: &LlmRequest) -> Result<String>;
}

pub struct FallbackOnly;

#[async_trait]
impl LlmBackend for FallbackOnly {
    fn id(&self) -> &str {
        "fallback"
    }
    fn model_name(&self) -> &str {
        ""
    }
    async fn complete(&self, _request: &LlmRequest) -> Result<String> {
        bail!("no local model configured")
    }
}

pub struct OllamaBackend {
    client: reqwest::Client,
    base: String,
    model: String,
}

impl OllamaBackend {
    pub fn new(base: &str, model: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(120))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            base: base.trim_end_matches('/').to_string(),
            model: model.to_string(),
        })
    }
}

#[async_trait]
impl LlmBackend for OllamaBackend {
    fn id(&self) -> &str {
        "ollama"
    }
    fn model_name(&self) -> &str {
        &self.model
    }
    async fn complete(&self, request: &LlmRequest) -> Result<String> {
        let (system, user) = prompt(request);
        let response = self
            .client
            .post(format!("{}/api/chat", self.base))
            .json(&json!({
                "model": self.model,
                "stream": false,
                "think": false,
                "format": "json",
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user}
                ]
            }))
            .send()
            .await
            .context("无法连接本机 Ollama")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!(
                "Ollama 拒绝请求 ({status})：{}",
                body.chars().take(240).collect::<String>()
            );
        }
        let value: Value = response.json().await.context("Ollama 响应不是 JSON")?;
        let content = value["message"]["content"]
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .context("Ollama 没有返回正文")?;
        Ok(content.to_string())
    }
}

pub fn prompt(request: &LlmRequest) -> (String, String) {
    let language = if request.language == "zh" {
        "Simplified Chinese"
    } else {
        "English"
    };
    let system = format!(
        "You edit a daily AI briefing for an e-ink device. \
         Write every natural-language field in {language}. \
         Be factual. Do not invent numbers, quotes, or events that are not in the notes. \
         Return only a JSON object, no markdown."
    );
    let mut user = format!(
        "Choose up to {} stories. For each, return id (copied exactly), title, summary (exactly 3 sentences), tts (a speakable script of about 40 to 70 words, no URLs, no markdown), and importance (integer 0-100).\n{{\"items\":[{{\"id\":\"...\",\"title\":\"...\",\"summary\":\"...\",\"tts\":\"...\",\"importance\":80}}]}}\n\nStories:\n",
        request.limit
    );
    for (index, cluster) in request.clusters.iter().enumerate() {
        let summary = first_sentences(&cluster.summary, 2);
        user.push_str(&format!(
            "{}. id={} sources={} points={} title={} note={}\n",
            index + 1,
            cluster.id,
            cluster.sources,
            cluster.popularity,
            cluster.title,
            summary
        ));
    }
    (system, user)
}

#[derive(Deserialize)]
struct LlmItem {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    tts: String,
    #[serde(default)]
    importance: Option<i64>,
}

pub fn extract_json_object(text: &str) -> Result<Value> {
    let stripped = strip_think(text);
    let stripped = strip_fence(&stripped);
    let start = stripped.find('{').context("模型回复里没有 JSON 对象")?;
    let end = stripped
        .rfind('}')
        .context("模型回复里没有完整的 JSON 对象")?;
    Ok(serde_json::from_str(&stripped[start..=end])?)
}

fn strip_think(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find("</think>") {
            rest = &rest[start + end + "</think>".len()..];
        } else {
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

fn strip_fence(text: &str) -> String {
    let trimmed = text.trim();
    let without = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    without.trim().trim_end_matches("```").trim().to_string()
}

pub struct ComposeResult {
    pub backend: String,
    pub model: String,
    pub warnings: Vec<String>,
    pub items: Vec<DigestItem>,
}

pub async fn compose(
    clusters: &[Cluster],
    language: &str,
    limit: usize,
    llm: &dyn LlmBackend,
    warnings: Vec<String>,
) -> ComposeResult {
    let limit = limit.clamp(1, 20);
    let pool: Vec<&Cluster> = clusters.iter().take(30).collect();
    let mut result_warnings = warnings;
    let briefs = pool
        .iter()
        .map(|cluster| ClusterBrief {
            id: cluster.id.clone(),
            title: cluster.title.clone(),
            summary: cluster.summary.chars().take(500).collect(),
            sources: cluster.source_names.join(", "),
            popularity: cluster.popularity,
            heuristic: cluster.heuristic,
        })
        .collect();
    let request = LlmRequest {
        language: language.to_string(),
        limit,
        clusters: briefs,
    };
    let drafted = match llm.complete(&request).await {
        Ok(text) => match items_from_model(&text, &pool, language) {
            Ok(items) if !items.is_empty() => Some(items),
            Ok(_) => {
                result_warnings.push("模型没有返回可用条目，已改用信源摘要。".into());
                None
            }
            Err(error) => {
                result_warnings.push(format!("模型回复无法解析，已改用信源摘要：{error}"));
                None
            }
        },
        Err(error) => {
            result_warnings.push(format!("未使用本地模型，已按信源权重和时间排序：{error}"));
            None
        }
    };
    let items = if let Some(mut items) = drafted {
        fill_from_heuristic(&mut items, &pool, language, limit);
        items.truncate(limit);
        (llm.id().to_string(), llm.model_name().to_string(), items)
    } else {
        let items = pool
            .iter()
            .take(limit)
            .map(|cluster| fallback_item(cluster, language))
            .collect();
        ("fallback".into(), String::new(), items)
    };
    ComposeResult {
        backend: items.0,
        model: items.1,
        warnings: result_warnings,
        items: items.2,
    }
}

fn items_from_model(text: &str, pool: &[&Cluster], language: &str) -> Result<Vec<DigestItem>> {
    let value = extract_json_object(text)?;
    let parsed: Vec<LlmItem> =
        serde_json::from_value(value["items"].clone()).context("JSON 缺少 items 数组")?;
    let mut ranked: Vec<(i64, usize, DigestItem)> = Vec::new();
    for (order, item) in parsed.into_iter().enumerate() {
        let Some(cluster) = pool.iter().find(|cluster| cluster.id == item.id) else {
            continue;
        };
        let mut digest = fallback_item(cluster, language);
        if !item.title.trim().is_empty() {
            digest.title = item.title.trim().to_string();
        }
        if !item.summary.trim().is_empty() {
            digest.summary = item.summary.trim().to_string();
        }
        if !item.tts.trim().is_empty() {
            digest.tts_script = item.tts.trim().to_string();
        }
        let importance = item.importance.unwrap_or(50).clamp(0, 100);
        digest.score = importance as f32;
        ranked.push((importance, order, digest));
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    Ok(ranked.into_iter().map(|(_, _, item)| item).collect())
}

fn fill_from_heuristic(
    items: &mut Vec<DigestItem>,
    pool: &[&Cluster],
    language: &str,
    limit: usize,
) {
    for cluster in pool {
        if items.len() >= limit {
            break;
        }
        if items.iter().any(|item| item.id == cluster.id) {
            continue;
        }
        items.push(fallback_item(cluster, language));
    }
}

pub fn fallback_item(cluster: &Cluster, language: &str) -> DigestItem {
    let summary = {
        let text = first_sentences(&cluster.summary, 3);
        if text.is_empty() {
            cluster.title.clone()
        } else {
            text
        }
    };
    let source = cluster
        .source_names
        .first()
        .cloned()
        .unwrap_or_else(|| "AI".into());
    let tts_script = if language == "zh" {
        format!(
            "来自{source}。{}。{summary}",
            cluster.title.trim_end_matches(['。', '.'])
        )
    } else {
        format!(
            "From {source}. {}. {summary}",
            cluster.title.trim_end_matches(['.', '。'])
        )
    };
    DigestItem {
        id: cluster.id.clone(),
        title: cluster.title.clone(),
        summary,
        tts_script,
        source,
        sources: cluster.source_names.clone(),
        url: cluster.url.clone(),
        published_at: cluster.published_at.clone(),
        score: cluster.heuristic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Article;

    fn sample_cluster(id: &str, title: &str, heuristic: f32) -> Cluster {
        Cluster {
            id: id.into(),
            title: title.into(),
            summary: format!(
                "{title}. Second sentence here. Third sentence here. Fourth stays out."
            ),
            url: format!("https://example.com/{id}"),
            source_names: vec!["OpenAI".into()],
            published_at: Some("2026-10-14T06:00:00Z".into()),
            popularity: 3,
            weight: 1.0,
            heuristic,
            articles: vec![Article {
                id: id.into(),
                source_id: "openai".into(),
                source_name: "OpenAI".into(),
                title: title.into(),
                url: format!("https://example.com/{id}"),
                summary: "note".into(),
                published_at: Some("2026-10-14T06:00:00Z".into()),
                popularity: 3,
                weight: 1.0,
            }],
        }
    }

    struct Scripted;

    #[async_trait]
    impl LlmBackend for Scripted {
        fn id(&self) -> &str {
            "ollama"
        }
        fn model_name(&self) -> &str {
            "qwen3"
        }
        async fn complete(&self, request: &LlmRequest) -> Result<String> {
            let items: Vec<_> = request
                .clusters
                .iter()
                .enumerate()
                .map(|(index, cluster)| {
                    json!({
                        "id": cluster.id,
                        "title": format!("[{}] {}", request.language, cluster.title),
                        "summary": "One. Two. Three.",
                        "tts": format!("Spoken {}", cluster.id),
                        "importance": if index == request.clusters.len() - 1 { 99 } else { 10 }
                    })
                })
                .collect();
            Ok(format!("```json\n{}\n```", json!({"items": items})))
        }
    }

    struct Broken;

    #[async_trait]
    impl LlmBackend for Broken {
        fn id(&self) -> &str {
            "ollama"
        }
        fn model_name(&self) -> &str {
            "qwen3"
        }
        async fn complete(&self, _request: &LlmRequest) -> Result<String> {
            bail!("connection refused")
        }
    }

    #[tokio::test]
    async fn model_reorders_and_strips_fences() {
        let clusters = vec![
            sample_cluster("aaa", "First story about models", 9.0),
            sample_cluster("bbb", "Second story about chips", 1.0),
        ];
        let composed = compose(&clusters, "zh", 2, &Scripted, Vec::new()).await;
        assert_eq!(composed.backend, "ollama");
        assert_eq!(composed.items[0].id, "bbb");
        assert!(composed.items[0].title.starts_with("[zh]"));
        assert_eq!(composed.items[0].tts_script, "Spoken bbb");
    }

    #[tokio::test]
    async fn fallback_uses_feed_sentences_when_the_model_is_down() {
        let clusters = vec![sample_cluster("aaa", "First story about models", 9.0)];
        let composed = compose(&clusters, "en", 12, &Broken, Vec::new()).await;
        assert_eq!(composed.backend, "fallback");
        assert!(composed
            .warnings
            .iter()
            .any(|line| line.contains("connection refused")));
        assert_eq!(
            composed.items[0].summary,
            "First story about models. Second sentence here. Third sentence here."
        );
        assert!(composed.items[0].tts_script.starts_with("From OpenAI."));
        let value =
            extract_json_object("<think>secret</think>\n```json\n{\"items\":[]}\n```").unwrap();
        assert!(value["items"].is_array());
    }
}
