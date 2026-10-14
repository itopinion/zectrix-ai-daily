//! Greedy clustering. The first article in a cluster is the representative
//! (callers pass articles already ordered by source weight). A later article
//! joins when its URL matches or its title is close enough.

use crate::model::{Article, Cluster};
use crate::text::{canonical_url, stable_id, titles_match};

pub fn cluster_articles(articles: Vec<Article>) -> Vec<Cluster> {
    let mut clusters: Vec<Cluster> = Vec::new();
    for article in articles {
        let url_key = canonical_url(&article.url);
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|cluster| belongs(cluster, &article, &url_key))
        {
            if !cluster
                .source_names
                .iter()
                .any(|name| name == &article.source_name)
            {
                cluster.source_names.push(article.source_name.clone());
            }
            cluster.popularity = cluster.popularity.max(article.popularity);
            cluster.weight = cluster.weight.max(article.weight);
            if article.summary.len() > cluster.summary.len() {
                cluster.summary = article.summary.clone();
            }
            if cluster.published_at.is_none() {
                cluster.published_at = article.published_at.clone();
            }
            cluster.articles.push(article);
        } else {
            let id = stable_id(if url_key.is_empty() {
                &article.title
            } else {
                &url_key
            });
            clusters.push(Cluster {
                id,
                title: article.title.clone(),
                summary: article.summary.clone(),
                url: article.url.clone(),
                source_names: vec![article.source_name.clone()],
                published_at: article.published_at.clone(),
                popularity: article.popularity,
                weight: article.weight,
                heuristic: 0.0,
                articles: vec![article],
            });
        }
    }
    clusters
}

fn belongs(cluster: &Cluster, article: &Article, url_key: &str) -> bool {
    if !url_key.is_empty()
        && cluster
            .articles
            .iter()
            .any(|existing| canonical_url(&existing.url) == url_key)
    {
        return true;
    }
    titles_match(&cluster.title, &article.title)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article(title: &str, url: &str, source: &str, weight: f32) -> Article {
        Article {
            id: format!("{source}-{title}"),
            source_id: source.into(),
            source_name: source.into(),
            title: title.into(),
            url: url.into(),
            summary: format!("{title} summary"),
            published_at: Some("2026-10-14T06:00:00Z".into()),
            popularity: 0,
            weight,
        }
    }

    #[test]
    fn same_story_from_two_sources_becomes_one_cluster() {
        let articles = vec![
            article(
                "OpenAI releases a pocket model",
                "https://openai.com/news/pocket",
                "OpenAI",
                1.3,
            ),
            article(
                "OpenAI releases a pocket model for phones",
                "https://www.theverge.com/pocket",
                "The Verge",
                1.0,
            ),
            article(
                "DeepMind predicts the weather",
                "https://deepmind.google/weather",
                "DeepMind",
                1.2,
            ),
        ];
        let clusters = cluster_articles(articles);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].source_names, vec!["OpenAI", "The Verge"]);
        assert_eq!(clusters[1].title, "DeepMind predicts the weather");
    }

    #[test]
    fn tracking_params_do_not_split_a_url() {
        let articles = vec![
            article(
                "One title here today",
                "https://example.com/a?utm_source=rss",
                "A",
                1.0,
            ),
            article(
                "Completely different headline",
                "https://example.com/a",
                "B",
                0.5,
            ),
        ];
        let clusters = cluster_articles(articles);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].articles.len(), 2);
    }
}
