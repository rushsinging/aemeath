use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use crate::tui::view_model::tool_name::tool_display_name;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::Path;

use super::common::truncate_json;
use super::policy::{DetailsPolicy, ResultPolicy, ResultRender};
use super::registry::lookup_display;

/// 返回某工具的渲染策略。未注册的工具回退到默认策略。
pub fn result_policy(name: &str) -> ResultPolicy {
    lookup_display(name)
        .map(|display| display.render_policy().result)
        .unwrap_or(ResultPolicy::Visible {
            max_lines: Some(5),
            render_kind: ResultRender::Plain,
            tail_mode: false,
        })
}

/// 该工具 result 的渲染类型（取自 `ToolDisplay::render_policy`，未注册回退 `Plain`）。
pub fn result_render_kind(name: &str) -> ResultRender {
    match result_policy(name) {
        ResultPolicy::Visible { render_kind, .. } => render_kind,
        _ => ResultRender::Plain,
    }
}

/// Format a tool call for sub-agent progress: header only, no result/details.
pub fn format_subagent_tool_header(
    name: &str,
    input: &serde_json::Value,
    workspace_root: Option<&Path>,
) -> String {
    lookup_display(name)
        .map(|display| display.header_for_subagent(input, workspace_root))
        .unwrap_or_else(|| {
            let raw = match input {
                serde_json::Value::String(s) => s.clone(),
                value => value.to_string(),
            };
            let preview = truncate_json(&raw);
            if preview.is_empty() {
                tool_display_name(name).to_string()
            } else {
                format!("{} {preview}", tool_display_name(name))
            }
        })
}

/// Format a tool call for human-friendly display.
pub fn format_tool_call(
    name: &str,
    raw_json: &str,
    result_payload: Option<&ToolResultPayload>,
    workspace_root: Option<&Path>,
) -> (Line<'static>, Vec<String>) {
    let parsed: serde_json::Value =
        serde_json::from_str(raw_json).unwrap_or(serde_json::Value::Null);

    if let Some(display) = lookup_display(name) {
        let header =
            display.format_header_line_with_result(&parsed, result_payload, workspace_root);
        let details = match display.render_policy().details {
            DetailsPolicy::Expanded => display.format_details(&parsed),
            DetailsPolicy::Hidden => vec![],
        };
        return (header, details);
    }

    let truncated = truncate_json(raw_json);
    (
        Line::from(vec![
            Span::raw("● "),
            Span::styled(
                tool_display_name(name).to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ),
        ]),
        vec![truncated],
    )
}
