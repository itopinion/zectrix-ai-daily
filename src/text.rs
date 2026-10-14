//! Small text helpers shared by parsing, fallback summaries, and the e-ink layout.
//! Widths are estimates: the Hub rasterizer is not in this repo, so the card
//! pre-wraps instead of trusting `width_limit` to wrap.

use sha2::{Digest, Sha256};
use url::Url;

pub fn hex_prefix(bytes: &[u8], nbytes: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(nbytes * 2);
    for byte in bytes.iter().take(nbytes) {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

pub fn stable_id(material: &str) -> String {
    hex_prefix(Sha256::digest(material.as_bytes()).as_slice(), 8)
}

pub fn unescape_basic(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some(end) = input[i..].find(';').map(|n| i + n) {
                let entity = &input[i..=end];
                let decoded = match entity {
                    "&amp;" => Some("&"),
                    "&lt;" => Some("<"),
                    "&gt;" => Some(">"),
                    "&quot;" => Some("\""),
                    "&apos;" => Some("'"),
                    "&nbsp;" => Some(" "),
                    _ => decode_numeric(entity),
                };
                if let Some(text) = decoded {
                    out.push_str(text);
                    i = end + 1;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn decode_numeric(entity: &str) -> Option<&'static str> {
    // Only the handful of numeric forms that show up in titles. Returning a
    // static string keeps the scanner simple; other numeric entities fall through.
    match entity {
        "&#39;" | "&#x27;" | "&#X27;" => Some("'"),
        "&#34;" | "&#x22;" => Some("\""),
        "&#160;" | "&#xa0;" => Some(" "),
        _ => None,
    }
}

pub fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    collapse_ws(&unescape_basic(&out))
}

pub fn collapse_ws(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending = false;
    for ch in input.chars() {
        if ch.is_whitespace() || ch == '\u{00a0}' {
            pending = !out.is_empty();
            continue;
        }
        if pending {
            out.push(' ');
            pending = false;
        }
        out.push(ch);
    }
    out
}

/// First `n` sentences. Decimal points and dotted initials are not boundaries.
pub fn first_sentences(text: &str, n: usize) -> String {
    let text = text.trim();
    if n == 0 || text.is_empty() {
        return String::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut count = 0;
    for i in 0..chars.len() {
        let ch = chars[i].1;
        if !is_sentence_end(ch) {
            continue;
        }
        if ch == '.' && is_internal_dot(&chars, i) {
            continue;
        }
        count += 1;
        if count >= n {
            let end = chars.get(i + 1).map(|(idx, _)| *idx).unwrap_or(text.len());
            return text[..end].trim().to_string();
        }
    }
    text.to_string()
}

fn is_sentence_end(ch: char) -> bool {
    matches!(ch, '.' | '。' | '!' | '！' | '?' | '？')
}

fn is_internal_dot(chars: &[(usize, char)], index: usize) -> bool {
    let prev = index.checked_sub(1).map(|i| chars[i].1);
    let next = chars.get(index + 1).map(|(_, ch)| *ch);
    let digit =
        prev.is_some_and(|ch| ch.is_ascii_digit()) && next.is_some_and(|ch| ch.is_ascii_digit());
    let initial = prev.is_some_and(|ch| ch.is_ascii_uppercase())
        && next.is_some_and(|ch| ch.is_ascii_uppercase() || ch == '.');
    digit || initial
}

pub fn char_px(ch: char, size: i32) -> i32 {
    if ch == ' ' {
        (size / 3).max(1)
    } else if ch.is_ascii() {
        ((size as f32) * 0.56).round() as i32
    } else {
        size
    }
}

pub fn text_px(text: &str, size: i32) -> i32 {
    text.chars().map(|ch| char_px(ch, size)).sum()
}

/// Greedy wrap. Latin breaks on spaces; CJK may break on any character.
pub fn wrap_lines(text: &str, size: i32, width: i32) -> Vec<String> {
    let text = collapse_ws(text);
    if text.is_empty() || width <= 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for unit in units(&text) {
        let unit_width = text_px(&unit, size);
        if !line.is_empty() && used + unit_width > width {
            lines.push(std::mem::take(&mut line).trim_end().to_string());
            used = 0;
        }
        if unit_width > width && line.is_empty() {
            // A single overlong token (a URL, a long CJK run already split).
            let (head, rest) = split_to_width(&unit, size, width);
            if !head.is_empty() {
                lines.push(head);
            }
            if !rest.trim().is_empty() {
                line = rest.trim_start().to_string();
                used = text_px(&line, size);
            }
            continue;
        }
        line.push_str(&unit);
        used += unit_width;
    }
    if !line.trim().is_empty() {
        lines.push(line.trim_end().to_string());
    }
    lines
}

fn units(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut latin = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '\'' || ch == '-' || ch == '/' {
            latin.push(ch);
            continue;
        }
        if !latin.is_empty() {
            out.push(std::mem::take(&mut latin));
        }
        out.push(ch.to_string());
    }
    if !latin.is_empty() {
        out.push(latin);
    }
    out
}

fn split_to_width(text: &str, size: i32, width: i32) -> (String, String) {
    let mut head = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = char_px(ch, size);
        if used + w > width && !head.is_empty() {
            let rest = text[head.len()..].to_string();
            return (head, rest);
        }
        head.push(ch);
        used += w;
    }
    (head, String::new())
}

pub fn ellipsize(text: &str, size: i32, width: i32) -> String {
    let text = collapse_ws(text);
    if text_px(&text, size) <= width {
        return text;
    }
    let ellipsis = "…";
    let budget = width - text_px(ellipsis, size);
    if budget <= 0 {
        return ellipsis.into();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = char_px(ch, size);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

pub fn normalize_compact(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

const STOP: &[&str] = &[
    "the", "a", "an", "of", "to", "and", "for", "in", "on", "with", "by", "from", "at", "its",
    "is", "as", "or", "new", "的", "了", "在", "与", "和",
];

/// Tokens used for cross-source clustering. Latin words plus CJK bigrams.
pub fn title_tokens(text: &str) -> std::collections::BTreeSet<String> {
    let lower: String = text.chars().flat_map(char::to_lowercase).collect();
    let mut tokens = std::collections::BTreeSet::new();
    let mut word = String::new();
    let mut cjk: Vec<char> = Vec::new();
    let flush_word = |word: &mut String, tokens: &mut std::collections::BTreeSet<String>| {
        if word.len() >= 2 && !STOP.contains(&word.as_str()) {
            tokens.insert(std::mem::take(word));
        } else {
            word.clear();
        }
    };
    let flush_cjk = |cjk: &mut Vec<char>, tokens: &mut std::collections::BTreeSet<String>| {
        if cjk.len() == 1 {
            tokens.insert(cjk[0].to_string());
        } else {
            for pair in cjk.windows(2) {
                tokens.insert(pair.iter().collect());
            }
        }
        cjk.clear();
    };
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            if !cjk.is_empty() {
                flush_cjk(&mut cjk, &mut tokens);
            }
            word.push(ch);
        } else if is_cjk(ch) {
            flush_word(&mut word, &mut tokens);
            cjk.push(ch);
        } else {
            flush_word(&mut word, &mut tokens);
            if !cjk.is_empty() {
                flush_cjk(&mut cjk, &mut tokens);
            }
        }
    }
    flush_word(&mut word, &mut tokens);
    if !cjk.is_empty() {
        flush_cjk(&mut cjk, &mut tokens);
    }
    tokens
}

fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{F900}'..='\u{FAFF}'
    )
}

pub fn titles_match(a: &str, b: &str) -> bool {
    let left = title_tokens(a);
    let right = title_tokens(b);
    if !left.is_empty() && !right.is_empty() {
        let inter = left.intersection(&right).count();
        let union = left.union(&right).count();
        if union > 0 && (inter as f32) / (union as f32) >= 0.55 {
            return true;
        }
    }
    let compact_a = normalize_compact(a);
    let compact_b = normalize_compact(b);
    let (short, long) = if compact_a.chars().count() <= compact_b.chars().count() {
        (compact_a, compact_b)
    } else {
        (compact_b, compact_a)
    };
    short.chars().count() >= 12 && long.contains(&short)
}

/// Drop tracking params and fragments so the same story URL clusters once.
pub fn canonical_url(input: &str) -> String {
    let Ok(mut parsed) = Url::parse(input.trim()) else {
        return input.trim().trim_end_matches('/').to_string();
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return String::new();
    }
    parsed.set_fragment(None);
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(key, _)| {
            let key = key.to_ascii_lowercase();
            !key.starts_with("utm_") && key != "ref" && key != "ref_src" && key != "fbclid"
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        let query = kept
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        parsed.set_query(Some(&query));
    }
    parsed.to_string().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_and_entities_collapse() {
        let raw = "<p>Tom &amp; Jerry&nbsp;say &#39;hi&#39;.</p><script>nope</script>";
        assert_eq!(strip_html(raw), "Tom & Jerry say 'hi'. nope");
    }

    #[test]
    fn three_sentences_keep_decimals() {
        let en = "Version 1.5 shipped. It is faster. It is smaller. It is done.";
        assert_eq!(
            first_sentences(en, 3),
            "Version 1.5 shipped. It is faster. It is smaller."
        );
        let zh = "第一句。第二句。第三句。第四句。";
        assert_eq!(first_sentences(zh, 3), "第一句。第二句。第三句。");
    }

    #[test]
    fn wrap_respects_width_and_cjk() {
        let lines = wrap_lines("alpha beta gamma delta epsilon", 16, 70);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(text_px(line, 16) <= 70, "{line}");
        }
        let zh = wrap_lines("机器之心今天发布了一篇关于世界模型的短讯", 16, 64);
        assert!(zh.len() > 1);
        for line in &zh {
            assert!(text_px(line, 16) <= 64, "{line}");
        }
    }

    #[test]
    fn titles_cluster_across_wording() {
        assert!(titles_match(
            "OpenAI releases a pocket model",
            "OpenAI releases a pocket model for phones"
        ));
        assert!(!titles_match(
            "DeepMind predicts the weather",
            "OpenAI releases a pocket model"
        ));
        assert!(titles_match(
            "深度求索发布新模型",
            "深度求索发布新模型，并开源权重"
        ));
    }
}
