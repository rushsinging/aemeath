use super::super::common::truncate_ellipsis_tail;
use crate::tui::render::theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// 截断路径，保留尾部（更有辨识度）。路径可含非 ASCII（如中文文件名）。
pub(crate) fn truncate_path(path: &str, max_width: usize) -> String {
    truncate_ellipsis_tail(path, max_width)
}

/// #1964：Header 主文案优先用 `goal`；旧快照缺 goal 时回退到技术载荷字段。
pub(crate) fn goal_or_fallback(goal: &str, fallback: &str) -> String {
    let trimmed = goal.trim();
    if !trimmed.is_empty() {
        trimmed.to_string()
    } else {
        fallback.trim().to_string()
    }
}

/// 构造 `ToolDisplay` 通用 header 文本：`{友好名} {goal|fallback}`。
pub(crate) fn format_goal_header(display_name: &str, goal: &str, fallback: &str) -> String {
    let text = goal_or_fallback(goal, fallback);
    if text.is_empty() {
        display_name.to_string()
    } else {
        format!("{display_name} {text}")
    }
}

/// 构造 `ToolDisplay` 通用 header line 模板：`<name> <path> [<suffix>]`。
pub(crate) fn build_header_line(name: &str, path: &str, suffix: &str) -> Line<'static> {
    let display_path = truncate_path(path, 60);
    let mut spans = vec![Span::styled(
        name.to_string(),
        Style::default().fg(theme::ACCENT_BRIGHT),
    )];
    if !display_path.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(display_path, Style::default().fg(theme::TEXT)));
    }
    if !suffix.is_empty() {
        spans.push(Span::styled(
            suffix.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
