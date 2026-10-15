//! NOTE4 uses short presses only: Up/Down navigate, Confirm opens or returns.
//! Long presses belong to the device recording UI. Web controls start continuous play.

use serde::Serialize;

pub const PAGE_SIZE: usize = 5;
pub const DETAIL_LINES: usize = 7;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailView {
    pub index: usize,
    pub page: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub cursor: usize,
    pub detail: Option<DetailView>,
    pub briefing: bool,
    pub notice: String,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            cursor: 0,
            detail: None,
            briefing: false,
            notice: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Opened,
    Closed,
    StopBriefing,
}

impl Board {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn page_index(&self) -> usize {
        self.cursor / PAGE_SIZE
    }

    pub fn handle(
        &mut self,
        key: &str,
        action: &str,
        item_count: usize,
        detail_pages: usize,
    ) -> Effect {
        // Never reinterpret recording gestures or unknown actions as navigation.
        if !action.eq_ignore_ascii_case("short") {
            return Effect::None;
        }
        if self.briefing {
            self.briefing = false;
            self.notice.clear();
            return Effect::StopBriefing;
        }
        let key = key.to_ascii_lowercase();
        if item_count == 0 {
            return Effect::None;
        }
        self.cursor = self.cursor.min(item_count - 1);
        match (self.detail.is_some(), key.as_str()) {
            (false, "up") => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                Effect::None
            }
            (false, "down") => {
                if self.cursor + 1 < item_count {
                    self.cursor += 1;
                }
                Effect::None
            }
            (false, "confirm") => {
                self.detail = Some(DetailView {
                    index: self.cursor,
                    page: 0,
                });
                self.notice.clear();
                Effect::Opened
            }
            (true, "up") => {
                if let Some(detail) = self.detail.as_mut() {
                    if detail.page > 0 {
                        detail.page -= 1;
                    }
                }
                Effect::None
            }
            (true, "down") => {
                if let Some(detail) = self.detail.as_mut() {
                    let last = detail_pages.saturating_sub(1);
                    if detail.page < last {
                        detail.page += 1;
                    }
                }
                Effect::None
            }
            (true, "confirm" | "back" | "left" | "cancel" | "return") => {
                self.detail = None;
                self.notice.clear();
                Effect::Closed
            }
            _ => Effect::None,
        }
    }

    pub fn open(&mut self, index: usize, item_count: usize) -> Effect {
        if item_count == 0 || index >= item_count {
            return Effect::None;
        }
        self.cursor = index;
        self.detail = Some(DetailView { index, page: 0 });
        self.briefing = false;
        Effect::Opened
    }


}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DeviceItem {
    pub index: usize,
    pub title: String,
    pub source: String,
    pub selected: bool,
    pub speaking: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DeviceDetail {
    pub index: usize,
    pub title: String,
    pub meta: String,
    pub lines: Vec<String>,
    pub page: usize,
    pub pages: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DeviceView {
    pub mode: String,
    pub date: String,
    pub notice: String,
    pub briefing: bool,
    pub cursor: usize,
    pub page: usize,
    pub pages: usize,
    pub footer: String,
    pub items: Vec<DeviceItem>,
    pub detail: Option<DeviceDetail>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging_select_and_back() {
        let mut board = Board::default();
        assert_eq!(board.handle("Down", "Short", 12, 1), Effect::None);
        assert_eq!(board.cursor, 1);
        for _ in 0..6 {
            board.handle("Down", "Short", 12, 1);
        }
        assert_eq!(board.cursor, 7);
        assert_eq!(board.page_index(), 1);
        assert_eq!(board.handle("Confirm", "Short", 12, 1), Effect::Opened);
        assert_eq!(board.detail.as_ref().unwrap().index, 7);
        board.handle("Down", "Short", 12, 3);
        assert_eq!(board.detail.as_ref().unwrap().page, 1);
        board.handle("Down", "Short", 12, 3);
        board.handle("Down", "Short", 12, 3);
        assert_eq!(board.detail.as_ref().unwrap().page, 2);
        board.handle("Back", "Short", 12, 3);
        assert!(board.detail.is_none());
        assert_eq!(board.cursor, 7);
    }

    #[test]
    fn recording_gestures_never_start_playback_or_change_navigation() {
        let mut board = Board::default();
        for in_detail in [false, true] {
            if in_detail { board.open(0, 4); }
            let before = board.clone();
            for action in ["Long", "Hold", "Held", "Release", "Unknown"] {
                assert_eq!(board.handle("Confirm", action, 4, 1), Effect::None);
                assert_eq!(board, before);
            }
        }
    }

    #[test]
    fn web_briefing_stops_on_first_short_press() {
        for key in ["Up", "Down", "Confirm"] {
            let mut board = Board { briefing: true, ..Board::default() };
            assert_eq!(board.handle(key, "Short", 4, 1), Effect::StopBriefing);
            assert!(!board.briefing);
            assert_eq!(board.cursor, 0);
        }
    }

    #[test]
    fn empty_list_ignores_keys() {
        let mut board = Board::default();
        assert_eq!(board.handle("Confirm", "Short", 0, 0), Effect::None);
        assert!(board.detail.is_none());
    }

    #[test]
    fn three_button_device_can_exit_detail_without_a_back_key() {
        let mut board = Board::default();
        board.handle("Down", "Short", 5, 1);
        assert_eq!(board.handle("Confirm", "Short", 5, 1), Effect::Opened);
        assert_eq!(board.handle("Confirm", "Short", 5, 1), Effect::Closed);
        assert!(board.detail.is_none());
        assert_eq!(board.cursor, 1);
    }
}
