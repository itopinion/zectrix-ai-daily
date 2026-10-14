//! Device chrome. The web page keeps its own dictionary in `web/app.js`
//! so the e-ink strings and the browser strings can diverge on purpose:
//! the device has room for one short footer line.

#[derive(Clone, Copy)]
pub struct Copy {
    pub masthead: &'static str,
    pub empty_title: &'static str,
    pub empty_body: &'static str,
    pub loading: &'static str,
    pub footer_list: &'static str,
    pub footer_detail: &'static str,
    pub footer_briefing: &'static str,
    pub speaking: &'static str,
    pub page: &'static str,
}

pub fn copy(language: &str) -> Copy {
    if language == "en" {
        Copy {
            masthead: "AI DAILY",
            empty_title: "No briefing yet",
            empty_body: "Refresh from the computer, or wait for 07:00.",
            loading: "Gathering today's AI briefing…",
            footer_list: "Up/Down select   Confirm read   Hold play",
            footer_detail: "Up/Down page   Back list   Confirm speak",
            footer_briefing: "Any key stops",
            speaking: "ON AIR",
            page: "p",
        }
    } else {
        Copy {
            masthead: "AI 日报",
            empty_title: "今日简报还没有生成",
            empty_body: "在电脑上刷新，或等到每天 07:00。",
            loading: "正在整理今日 AI 简报…",
            footer_list: "上下选择    确认阅读    长按连播",
            footer_detail: "上下翻页    返回列表    确认朗读",
            footer_briefing: "任意键停止",
            speaking: "播报中",
            page: "页",
        }
    }
}
