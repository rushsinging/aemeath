use super::constants::{CLOSE_TAG, OPEN_TAG};
use std::borrow::Cow;

pub fn strip_system_reminder_envelope(text: &str) -> Cow<'_, str> {
    let trimmed = text.trim();
    let Some(after_open) = strip_open_tag(trimmed) else {
        return Cow::Borrowed(text);
    };
    let Some(inner) = after_open.strip_suffix(CLOSE_TAG) else {
        return Cow::Borrowed(text);
    };
    Cow::Owned(inner.trim().to_string())
}

/// envelope 开标签剥离：兼容裸 `<system-reminder>` 与带属性形态
/// `<system-reminder kind=".." version=".." at=".." seq="..">`（#1695
/// envelope 化后开标签携带属性——此前只认裸标签，带属性 reminder 在
/// TUI 渲染中不被剥离，resume 后历史落盘的 reminder 原样显示）。
fn strip_open_tag(text: &str) -> Option<&str> {
    if let Some(rest) = text.strip_prefix(OPEN_TAG) {
        return Some(rest);
    }
    if text.starts_with("<system-reminder") {
        let close = text.find('>')?;
        return text.get(close + 1..);
    }
    None
}

pub fn strip_system_reminder_envelope_owned(text: String) -> String {
    match strip_system_reminder_envelope(&text) {
        Cow::Borrowed(_) => text,
        Cow::Owned(stripped) => stripped,
    }
}

#[cfg(test)]
#[path = "system_reminder_tests.rs"]
mod tests;
