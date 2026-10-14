//! NOTE4 navigation. Five stories per page. Up and down move a cursor;
//! confirm opens the story; back returns to the list. A long confirm starts
//! the radio-style briefing. While that is playing, any key stops it, except
//! the confirm release that ends the long press itself.

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
    /// False during the brief moment after a long-press starts, so the key
    /// release does not immediately stop playback.
    pub stop_armed: bool,
    pub notice: String,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            cursor: 0,
            detail: None,
            briefing: false,
            stop_armed: false,
            notice: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Opened,
    Speak,
    StartBriefing,
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
        if self.briefing {
            let confirm = key.eq_ignore_ascii_case("confirm");
            if !self.stop_armed && confirm {
                return Effect::None;
            }
            self.briefing = false;
            self.stop_armed = false;
            self.notice.clear();
            return Effect::StopBriefing;
        }
        let key = key.to_ascii_lowercase();
        let long = matches!(
            action.to_ascii_lowercase().as_str(),
            "long" | "hold" | "held"
        );
        if item_count == 0 {
            return Effect::None;
        }
        self.cursor = self.cursor.min(item_count - 1);
        match (self.detail.is_some(), key.as_str(), long) {
            (false, "up", false) => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                Effect::None
            }
            (false, "down", false) => {
                if self.cursor + 1 < item_count {
                    self.cursor += 1;
                }
                Effect::None
            }
            (false, "confirm", false) => {
                self.detail = Some(DetailView {
                    index: self.cursor,
                    page: 0,
                });
                self.notice.clear();
                Effect::Opened
            }
            (false, "confirm", true) => {
                self.briefing = true;
                self.stop_armed = false;
                self.notice.clear();
                Effect::StartBriefing
            }
            (true, "up", false) => {
                if let Some(detail) = self.detail.as_mut() {
                    if detail.page > 0 {
                        detail.page -= 1;
                    }
                }
                Effect::None
            }
            (true, "down", false) => {
                if let Some(detail) = self.detail.as_mut() {
                    let last = detail_pages.saturating_sub(1);
                    if detail.page < last {
                        detail.page += 1;
                    }
                }
                Effect::None
            }
            (true, "confirm", false) => Effect::Speak,
            (true, "back" | "left" | "cancel" | "return", _) => {
                self.detail = None;
                self.notice.clear();
                Effect::None
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

    pub fn arm_stop(&mut self) {
        if self.briefing {
            self.stop_armed = true;
        }
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
    fn long_press_starts_and_the_release_does_not_stop() {
        let mut board = Board::default();
        assert_eq!(board.handle("Confirm", "Long", 4, 1), Effect::StartBriefing);
        assert!(board.briefing);
        assert_eq!(board.handle("Confirm", "Short", 4, 1), Effect::None);
        assert!(board.briefing);
        board.arm_stop();
        assert_eq!(board.handle("Down", "Short", 4, 1), Effect::StopBriefing);
        assert!(!board.briefing);
        assert_eq!(board.cursor, 0);
    }

    #[test]
    fn empty_list_ignores_keys() {
        let mut board = Board::default();
        assert_eq!(board.handle("Confirm", "Short", 0, 0), Effect::None);
        assert!(board.detail.is_none());
    }
}
