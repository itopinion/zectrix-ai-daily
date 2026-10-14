//! 400×300 card passed to `POST /runtime/render`.
//!
//! Assumption, taken from the vocabulary app because the Hub repo was not
//! readable here: the canvas is 400×300, `y` is the vertical center of a text
//! run, and elements are `rect`, `hline`, and `text` with `width_limit`,
//! `align`, and `weight`. Text is pre-wrapped. A `rect` is treated as a
//! stroke, matching the vocabulary frame, so selection is a marker glyph
//! rather than a filled bar.

use crate::device::{Board, DeviceDetail, DeviceItem, DeviceView, DETAIL_LINES, PAGE_SIZE};
use crate::i18n::{self, Copy};
use crate::model::{Digest, DigestItem};
use crate::text::{ellipsize, wrap_lines};
use chrono::{DateTime, Local};
use serde_json::{json, Value};

pub const CANVAS_W: i32 = 400;
pub const CANVAS_H: i32 = 300;

pub fn card(board: &Board, digest: Option<&Digest>, loading: bool, language: &str) -> Value {
    let copy = i18n::copy(language);
    let mut elements = vec![
        json!({"type":"rect","x":0,"y":0,"w":399,"h":299}),
        json!({"type":"hline","x":8,"y":34,"w":384}),
        json!({"type":"hline","x":8,"y":262,"w":384}),
    ];
    let date = digest.map(|item| item.date.clone()).unwrap_or_default();
    text(
        &mut elements,
        copy.masthead,
        16,
        18,
        16,
        160,
        "left",
        "bold",
    );
    if loading && digest.map(|item| item.items.is_empty()).unwrap_or(true) {
        if !date.is_empty() {
            text(&mut elements, &date, 384, 18, 12, 180, "right", "regular");
        }
        text(
            &mut elements,
            copy.loading,
            200,
            150,
            16,
            360,
            "center",
            "regular",
        );
        text(
            &mut elements,
            copy.footer_list,
            200,
            280,
            12,
            380,
            "center",
            "regular",
        );
        return document(elements);
    }
    let Some(digest) = digest.filter(|item| !item.items.is_empty()) else {
        if !date.is_empty() {
            text(&mut elements, &date, 384, 18, 12, 180, "right", "regular");
        }
        text(
            &mut elements,
            copy.empty_title,
            200,
            132,
            18,
            360,
            "center",
            "bold",
        );
        text(
            &mut elements,
            copy.empty_body,
            200,
            168,
            14,
            360,
            "center",
            "regular",
        );
        text(
            &mut elements,
            copy.footer_list,
            200,
            280,
            12,
            380,
            "center",
            "regular",
        );
        return document(elements);
    };
    if let Some(detail) = &board.detail {
        draw_detail(&mut elements, board, digest, detail.index, &copy);
    } else {
        draw_list(&mut elements, board, digest, &copy);
    }
    document(elements)
}

pub fn view_model(
    board: &Board,
    digest: Option<&Digest>,
    loading: bool,
    language: &str,
) -> DeviceView {
    let copy = i18n::copy(language);
    let date = digest.map(|item| item.date.clone()).unwrap_or_default();
    if loading && digest.map(|item| item.items.is_empty()).unwrap_or(true) {
        return DeviceView {
            mode: "loading".into(),
            date,
            notice: board.notice.clone(),
            briefing: false,
            cursor: 0,
            page: 0,
            pages: 0,
            footer: copy.loading.into(),
            items: Vec::new(),
            detail: None,
        };
    }
    let Some(digest) = digest.filter(|item| !item.items.is_empty()) else {
        return DeviceView {
            mode: "empty".into(),
            date,
            notice: board.notice.clone(),
            briefing: board.briefing,
            cursor: 0,
            page: 0,
            pages: 0,
            footer: copy.empty_body.into(),
            items: Vec::new(),
            detail: None,
        };
    };
    if let Some(detail) = &board.detail {
        let item = digest.items.get(detail.index);
        let pages = item.map(|item| detail_page_count(item)).unwrap_or(1);
        let lines = item
            .map(|item| detail_lines(item, detail.page))
            .unwrap_or_default();
        DeviceView {
            mode: "detail".into(),
            date,
            notice: board.notice.clone(),
            briefing: board.briefing,
            cursor: board.cursor,
            page: detail.page,
            pages,
            footer: if board.briefing {
                copy.footer_briefing.into()
            } else {
                copy.footer_detail.into()
            },
            items: Vec::new(),
            detail: item.map(|item| DeviceDetail {
                index: detail.index,
                title: item.title.clone(),
                meta: meta_line(item),
                lines,
                page: detail.page,
                pages,
            }),
        }
    } else {
        let pages = digest.items.len().div_ceil(PAGE_SIZE).max(1);
        let start = board.page_index() * PAGE_SIZE;
        let items = digest
            .items
            .iter()
            .enumerate()
            .skip(start)
            .take(PAGE_SIZE)
            .map(|(index, item)| DeviceItem {
                index,
                title: item.title.clone(),
                source: meta_line(item),
                selected: index == board.cursor,
                speaking: board.briefing && index == board.cursor,
            })
            .collect();
        DeviceView {
            mode: "list".into(),
            date,
            notice: board.notice.clone(),
            briefing: board.briefing,
            cursor: board.cursor,
            page: board.page_index(),
            pages,
            footer: if board.briefing {
                copy.footer_briefing.into()
            } else {
                copy.footer_list.into()
            },
            items,
            detail: None,
        }
    }
}

fn draw_list(elements: &mut Vec<Value>, board: &Board, digest: &Digest, copy: &Copy) {
    let total_pages = digest.items.len().div_ceil(PAGE_SIZE).max(1);
    text(
        elements,
        &format!(
            "{}  {}/{}",
            digest.date,
            board.page_index() + 1,
            total_pages
        ),
        384,
        18,
        12,
        210,
        "right",
        "regular",
    );
    let start = board.page_index() * PAGE_SIZE;
    for slot in 0..PAGE_SIZE {
        let Some(item) = digest.items.get(start + slot) else {
            break;
        };
        let top = 40 + slot as i32 * 42;
        let index = start + slot;
        let selected = index == board.cursor;
        let marker = if board.briefing && selected {
            copy.speaking
        } else if selected {
            "▸"
        } else {
            " "
        };
        let title = ellipsize(&format!("{marker} {}", item.title), 15, 368);
        text(
            elements,
            &title,
            16,
            top + 12,
            15,
            368,
            "left",
            if selected { "bold" } else { "regular" },
        );
        text(
            elements,
            &ellipsize(&meta_line(item), 12, 360),
            28,
            top + 30,
            12,
            350,
            "left",
            "regular",
        );
    }
    let footer = if board.briefing {
        copy.footer_briefing
    } else {
        copy.footer_list
    };
    text(elements, footer, 200, 280, 12, 380, "center", "regular");
}

fn draw_detail(
    elements: &mut Vec<Value>,
    board: &Board,
    digest: &Digest,
    index: usize,
    copy: &Copy,
) {
    let Some(item) = digest.items.get(index) else {
        text(
            elements,
            copy.empty_title,
            200,
            150,
            16,
            360,
            "center",
            "regular",
        );
        return;
    };
    let page = board.detail.as_ref().map(|detail| detail.page).unwrap_or(0);
    let pages = detail_page_count(item);
    text(
        elements,
        &format!("{}  {}/{}", digest.date, index + 1, digest.items.len()),
        384,
        18,
        12,
        220,
        "right",
        "regular",
    );
    let title_lines = wrap_lines(&item.title, 18, 360);
    text(
        elements,
        title_lines.first().map(String::as_str).unwrap_or(""),
        20,
        52,
        18,
        360,
        "left",
        "bold",
    );
    text(
        elements,
        title_lines.get(1).map(String::as_str).unwrap_or(""),
        20,
        74,
        18,
        360,
        "left",
        "bold",
    );
    text(
        elements,
        &ellipsize(&meta_line(item), 12, 360),
        20,
        98,
        12,
        360,
        "left",
        "regular",
    );
    elements.push(json!({"type":"hline","x":20,"y":112,"w":360}));
    for (row, line) in detail_lines(item, page).into_iter().enumerate() {
        text(
            elements,
            &line,
            20,
            132 + row as i32 * 17,
            14,
            360,
            "left",
            "regular",
        );
    }
    let numbered = format!("{}  {}/{}", copy.footer_detail, page + 1, pages);
    let footer = if board.briefing {
        copy.footer_briefing
    } else if pages > 1 {
        numbered.as_str()
    } else {
        copy.footer_detail
    };
    text(elements, footer, 200, 280, 12, 380, "center", "regular");
}

pub fn detail_page_count(item: &DigestItem) -> usize {
    wrap_lines(&item.summary, 14, 360)
        .len()
        .div_ceil(DETAIL_LINES)
        .max(1)
}

fn detail_lines(item: &DigestItem, page: usize) -> Vec<String> {
    let lines = wrap_lines(&item.summary, 14, 360);
    lines
        .into_iter()
        .skip(page * DETAIL_LINES)
        .take(DETAIL_LINES)
        .collect()
}

fn meta_line(item: &DigestItem) -> String {
    let when = item
        .published_at
        .as_deref()
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|stamp| {
            stamp
                .with_timezone(&Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default();
    let sources = if item.sources.len() > 1 {
        format!("{} +{}", item.source, item.sources.len() - 1)
    } else {
        item.source.clone()
    };
    if when.is_empty() {
        sources
    } else {
        format!("{sources} · {when}")
    }
}

fn text(
    elements: &mut Vec<Value>,
    content: &str,
    x: i32,
    y: i32,
    size: i32,
    width: i32,
    align: &str,
    weight: &str,
) {
    if content.is_empty() {
        return;
    }
    elements.push(json!({
        "type": "text",
        "content": content,
        "x": x,
        "y": y,
        "size": size,
        "width_limit": width,
        "align": align,
        "weight": weight,
        "font": "default"
    }));
}

fn document(elements: Vec<Value>) -> Value {
    json!({"canvas":{"w":CANVAS_W,"h":CANVAS_H},"elements":elements})
}

pub fn detail_pages_for(digest: Option<&Digest>, index: usize) -> usize {
    digest
        .and_then(|digest| digest.items.get(index))
        .map(detail_page_count)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Digest, DigestItem};

    fn digest() -> Digest {
        let mut digest = Digest::empty("2026-10-14", "zh");
        digest.items = (0..6)
            .map(|index| DigestItem {
                id: format!("id{index}"),
                title: format!("故事标题 {index} 足够用来占满一行"),
                summary: "第一句说明发生了什么。第二句补充来源里的事实。第三句说明它为什么值得听。第四句只在翻页后出现，避免第一页被撑破。".repeat(8),
                tts_script: "播报".into(),
                source: "量子位".into(),
                sources: vec!["量子位".into()],
                url: "https://example.com".into(),
                published_at: Some("2026-10-14T01:00:00Z".into()),
                score: 1.0,
            })
            .collect();
        digest
    }

    #[test]
    fn list_marks_the_cursor_and_detail_paginates() {
        let digest = digest();
        let mut board = Board::default();
        board.cursor = 5;
        let list = card(&board, Some(&digest), false, "zh");
        let rendered = list.to_string();
        assert!(rendered.contains("故事标题 5"));
        assert!(!rendered.contains("故事标题 0"));
        assert!(rendered.contains("2/2"));
        board.detail = Some(crate::device::DetailView { index: 5, page: 0 });
        let detail = card(&board, Some(&digest), false, "zh");
        assert!(detail.to_string().contains("第一句"));
        assert!(detail_page_count(&digest.items[0]) > 1);
        let model = view_model(&board, Some(&digest), false, "en");
        assert_eq!(model.mode, "detail");
    }

    #[test]
    fn empty_and_loading_states_are_distinct() {
        let board = Board::default();
        assert!(card(&board, None, true, "en")
            .to_string()
            .contains("Gathering"));
        assert!(card(&board, None, false, "zh")
            .to_string()
            .contains("还没有生成"));
    }
}
