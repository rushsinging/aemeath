use crate::tui::render::display::safe_text;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use serde::de::DeserializeOwned;
use std::path::Path;

/// 从 `payload.content` 反序列化到 typed struct。
///
/// 返回 `None` 当 payload 缺失、content 为 Null、或反序列化失败。
/// 由 tool_impls / task_impls 共享（issue #486：TaskUpdate 从 result 取 subject）。
pub(super) fn typed_data<T: DeserializeOwned>(payload: Option<&ToolResultPayload>) -> Option<T> {
    let payload = payload?;
    if payload.content.is_null() {
        return None;
    }
    serde_json::from_value(payload.content.clone()).ok()
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

/// 将路径相对于 `workspace_root` 显示：能 `strip_prefix` 成功时返回相对路径（无 `./` 前缀），
/// 否则原样返回。`workspace_root` 为 `None` 时原样返回。
///
/// **不 canonicalize**——路径不存在时 canonicalize 会失败，issue #342 要求「路径不存在不破坏展示」。
/// 仅做纯字符串层面的前缀剥离，与 PolicyEngine 的 canonicalize 保证互补（执行链路已规整）。
///
/// 路径等于 `workspace_root` 本身（strip 成功且为空）时返回 `.`。
pub(super) fn display_path(raw: &str, workspace_root: Option<&Path>) -> String {
    let Some(root) = workspace_root else {
        return raw.to_string();
    };
    if raw.is_empty() {
        return raw.to_string();
    }
    match Path::new(raw).strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.display().to_string(),
        Err(_) => raw.to_string(),
    }
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod tests;
