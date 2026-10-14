//! Importance without a model: source weight, recency, HN points, and how
//! many outlets carried the story. The LLM path may reorder this list; it
//! does not replace the score when the model is absent.

use crate::model::Cluster;
use chrono::{DateTime, Utc};

pub fn rank(mut clusters: Vec<Cluster>, now: DateTime<Utc>) -> Vec<Cluster> {
    for cluster in &mut clusters {
        cluster.heuristic = score(cluster, now);
    }
    clusters.sort_by(|a, b| {
        b.heuristic
            .partial_cmp(&a.heuristic)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    clusters
}

pub fn score(cluster: &Cluster, now: DateTime<Utc>) -> f32 {
    let age_hours = cluster
        .published_at
        .as_deref()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|stamp| {
            let minutes = (now - stamp.with_timezone(&Utc)).num_minutes();
            (minutes as f32 / 60.0).max(0.0)
        })
        .unwrap_or(36.0);
    let recency = (1.0 - age_hours / 36.0).clamp(0.0, 1.0);
    let popularity = (cluster.popularity as f32 + 1.0).ln();
    let outlets = (cluster.articles.len() as f32).ln_1p();
    cluster.weight * 4.0 + recency * 3.0 + popularity * 0.85 + outlets * 1.2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Article;

    fn cluster(title: &str, weight: f32, hours_ago: i64, points: u32, outlets: usize) -> Cluster {
        let published = (Utc::now() - chrono::Duration::hours(hours_ago)).to_rfc3339();
        let articles = (0..outlets)
            .map(|index| Article {
                id: format!("{title}-{index}"),
                source_id: format!("s{index}"),
                source_name: format!("Source {index}"),
                title: title.into(),
                url: format!("https://example.com/{title}/{index}"),
                summary: "summary".into(),
                published_at: Some(published.clone()),
                popularity: points,
                weight,
            })
            .collect();
        Cluster {
            id: title.into(),
            title: title.into(),
            summary: "summary".into(),
            url: format!("https://example.com/{title}"),
            source_names: (0..outlets)
                .map(|index| format!("Source {index}"))
                .collect(),
            published_at: Some(published),
            popularity: points,
            weight,
            heuristic: 0.0,
            articles,
        }
    }

    #[test]
    fn recent_heavy_source_outranks_old_light_one() {
        let now = Utc::now();
        let ranked = rank(
            vec![
                cluster("old blog", 0.4, 20, 0, 1),
                cluster("fresh lab", 1.3, 1, 0, 1),
            ],
            now,
        );
        assert_eq!(ranked[0].title, "fresh lab");
        assert!(ranked[0].heuristic > ranked[1].heuristic);
    }

    #[test]
    fn hn_points_and_repeat_coverage_lift_a_story() {
        let now = Utc::now();
        let quiet = score(&cluster("quiet", 1.0, 2, 0, 1), now);
        let loud = score(&cluster("loud", 1.0, 2, 400, 3), now);
        assert!(loud > quiet);
    }
}
