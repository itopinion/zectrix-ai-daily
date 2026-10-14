//! RSS 2.0 and Atom parsers. Both OpenAI-style RSS and arXiv Atom go through
//! [`parse_feed`]. HTML pages that answer 200 are rejected so a dead "feed"
//! URL cannot be stored as a story.

use crate::text::{collapse_ws, strip_html};
use anyhow::{bail, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Clone, Debug, PartialEq)]
pub struct FeedItem {
    pub title: String,
    pub url: String,
    pub summary: String,
    pub published_raw: String,
}

pub fn parse_feed(xml: &str) -> Result<Vec<FeedItem>> {
    let trimmed = xml.trim_start();
    let head = trimmed.get(..64).unwrap_or(trimmed).to_ascii_lowercase();
    if head.starts_with("<!doctype html") || head.starts_with("<html") {
        bail!("response is HTML, not an RSS or Atom feed");
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut items = Vec::new();
    let mut current: Option<Partial> = None;
    let mut field = String::new();

    loop {
        let event = reader.read_event_into(&mut buf)?;
        match event {
            Event::Start(element) => {
                let name = local_name(element.local_name().as_ref());
                if let Some(item) = current.as_mut() {
                    if name == "link" {
                        capture_link(item, element.attributes());
                    }
                }
                if name == "item" || name == "entry" {
                    current = Some(Partial::default());
                }
                if is_text_field(&name) {
                    field.clear();
                }
                stack.push(name);
            }
            Event::Empty(element) => {
                let name = local_name(element.local_name().as_ref());
                if name == "link" {
                    if let Some(item) = current.as_mut() {
                        capture_link(item, element.attributes());
                    }
                }
            }
            Event::Text(text) => {
                field.push_str(&text.unescape().unwrap_or_default());
            }
            Event::CData(text) => {
                field.push_str(&String::from_utf8_lossy(text.as_ref()));
            }
            Event::End(_) => {
                let name = stack.pop().unwrap_or_default();
                if let Some(item) = current.as_mut() {
                    if is_text_field(&name) {
                        let value = collapse_ws(&field);
                        match name.as_str() {
                            "title" if item.title.is_empty() => item.title = strip_html(&value),
                            "description" | "summary" | "encoded" | "content" => {
                                let clean = strip_html(&value);
                                if clean.len() > item.summary.len() {
                                    item.summary = clean;
                                }
                            }
                            "pubdate" | "published" | "updated" | "date"
                                if item.published.is_empty() =>
                            {
                                item.published = value;
                            }
                            "link" if item.url.is_empty() => item.url = value,
                            "guid" | "id" if item.guid.is_empty() => item.guid = value,
                            _ => {}
                        }
                        field.clear();
                    }
                }
                if name == "item" || name == "entry" {
                    if let Some(item) = current.take() {
                        if let Some(ready) = item.finish() {
                            items.push(ready);
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if items.is_empty() && !stack.is_empty() {
        bail!("feed ended inside an element");
    }
    if items.is_empty() {
        bail!("feed contained no items");
    }
    Ok(items)
}

fn is_text_field(name: &str) -> bool {
    matches!(
        name,
        "title"
            | "description"
            | "summary"
            | "encoded"
            | "content"
            | "pubdate"
            | "published"
            | "updated"
            | "date"
            | "link"
            | "guid"
            | "id"
    )
}

fn local_name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_ascii_lowercase()
}

fn capture_link(item: &mut Partial, attributes: quick_xml::events::attributes::Attributes) {
    let mut href = String::new();
    let mut rel = String::new();
    for attr in attributes.flatten() {
        let key = local_name(attr.key.local_name().as_ref());
        let value = attr.unescape_value().unwrap_or_default().to_string();
        match key.as_str() {
            "href" => href = value,
            "rel" => rel = value,
            _ => {}
        }
    }
    if href.is_empty() {
        return;
    }
    let alternate = rel.is_empty() || rel == "alternate";
    if item.url.is_empty() || alternate {
        item.url = href;
        item.link_locked = rel == "alternate";
    }
}

#[derive(Default)]
struct Partial {
    title: String,
    url: String,
    summary: String,
    published: String,
    guid: String,
    link_locked: bool,
}

impl Partial {
    fn finish(self) -> Option<FeedItem> {
        let title = self.title.trim().to_string();
        if title.is_empty() {
            return None;
        }
        let url = if self.url.starts_with("http://") || self.url.starts_with("https://") {
            self.url
        } else if self.guid.starts_with("http://") || self.guid.starts_with("https://") {
            self.guid
        } else {
            String::new()
        };
        let _ = self.link_locked;
        Some(FeedItem {
            title,
            url,
            summary: self.summary,
            published_raw: self.published,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_item_with_cdata_and_html() {
        let xml = r#"<?xml version="1.0"?>
        <rss version="2.0"><channel><title>Example</title>
          <item>
            <title><![CDATA[OpenAI ships a tiny model]]></title>
            <link>https://example.com/tiny?utm_source=rss</link>
            <pubDate>Tue, 13 Oct 2026 11:05:39 +0000</pubDate>
            <description><![CDATA[<p>First sentence. Second sentence. Third sentence. Fourth.</p>]]></description>
          </item>
        </channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "OpenAI ships a tiny model");
        assert!(items[0].url.contains("example.com/tiny"));
        assert!(items[0].summary.starts_with("First sentence."));
        assert_eq!(items[0].published_raw, "Tue, 13 Oct 2026 11:05:39 +0000");
    }

    #[test]
    fn atom_entry_uses_alternate_link() {
        let xml = r#"<?xml version="1.0"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
          <title>AI</title>
          <entry>
            <title>Verge story</title>
            <link href="https://example.com/comments" rel="replies"/>
            <link href="https://example.com/story" rel="alternate"/>
            <updated>2026-10-13T09:00:00Z</updated>
            <summary>A short summary.</summary>
          </entry>
        </feed>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items[0].url, "https://example.com/story");
        assert_eq!(items[0].published_raw, "2026-10-13T09:00:00Z");
        assert_eq!(items[0].summary, "A short summary.");
    }

    #[test]
    fn arxiv_atom_and_html_rejection() {
        let xml = r#"<?xml version="1.0"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
          <entry>
            <id>http://arxiv.org/abs/2610.00001</id>
            <title>A paper about language models</title>
            <published>2026-10-13T17:00:00Z</published>
            <summary>Abstract sentence one. Abstract sentence two.</summary>
            <link href="https://arxiv.org/abs/2610.00001" rel="alternate" type="text/html"/>
          </entry>
        </feed>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items[0].url, "https://arxiv.org/abs/2610.00001");
        assert!(parse_feed("<!DOCTYPE html><html><title>nope</title></html>").is_err());
    }
}
