use crate::tui::render::display::safe_text;

pub(super) fn str_arg<'a>(input: &'a serde_json::Value, key: &str, default: &'a str) -> &'a str {
    input
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or(default)
}

pub(super) fn u64_arg(input: &serde_json::Value, key: &str) -> Option<u64> {
    input.get(key).and_then(|value| value.as_u64())
}

pub(super) fn bool_arg(input: &serde_json::Value, key: &str, default: bool) -> bool {
    input
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

pub(super) fn file_path(input: &serde_json::Value) -> &str {
    str_arg(input, "file_path", "?")
}

pub(super) fn truncate_ellipsis(text: &str, max_width: usize) -> String {
    if text.len() > max_width {
        let (prefix, _) = safe_text::truncate_unicode_width(text, max_width);
        format!("{}...", prefix)
    } else {
        text.to_string()
    }
}

/// 尾部截断：保留末尾、前缀加 `...`（char 边界安全，与 `truncate_ellipsis` 对称）。
/// 用于路径等「尾部更有辨识度」的场景。
pub(super) fn truncate_ellipsis_tail(text: &str, max_width: usize) -> String {
    if text.len() > max_width {
        let (suffix, _) = safe_text::truncate_last_unicode_width(text, max_width);
        format!("...{}", suffix)
    } else {
        text.to_string()
    }
}

pub(super) fn truncate_json(raw: &str) -> String {
    truncate_ellipsis(raw, 100)
}
