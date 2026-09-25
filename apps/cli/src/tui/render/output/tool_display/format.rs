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
#[cfg(test)]
pub fn result_render_kind(name: &str) -> ResultRender {
    match result_policy(name) {
        ResultPolicy::Visible { render_kind, .. } => render_kind,
        _ => ResultRender::Plain,
    }
}

/// Format a tool call for sub-agent activity: header only, no result/details.
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

/// #1666：执行耗时展示格式（自动进位，风格对齐 spinner 的空格分隔/零值省略）：
/// `850ms`（<1s）→ `1.24s`（<59.5s，百分秒）→ `1m 5s`（<60min，秒四舍五入
/// 后 ≥60s 升档，NEVER 显示 60.00s）→ `1h 2m`（≥60min，分钟四舍五入升档，
/// 零分钟省略为 `1h`）。
pub(super) fn format_call_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        return format!("{duration_ms}ms");
    }
    if duration_ms < 59_500 {
        // 上限 59_499 舍入到百分秒后 < 60.00s，避免秒档显示 60.00s。
        return format!("{:.2}s", duration_ms as f64 / 1_000.0);
    }
    // 自动进位：四舍五入到整秒后跨档（59_600ms → 1m 0s；3_599_700ms → 1h）。
    let total_seconds = (duration_ms + 500) / 1_000;
    let total_minutes = total_seconds / 60;
    if total_minutes < 60 {
        return format!("{}m {}s", total_minutes, total_seconds % 60);
    }
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if minutes == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {minutes}m")
    }
}

/// #1666：header 尾部追加 supervisor 耗时后缀 ` · 1.24s`（muted 色）；
/// duration 为 None（非 supervisor 路径 / 旧数据）时原样返回，NEVER 渲染占位。
fn append_duration_suffix(
    mut line: Line<'static>,
    result_payload: Option<&ToolResultPayload>,
) -> Line<'static> {
    if let Some(duration_ms) = result_payload.and_then(|payload| payload.duration_ms) {
        line.spans.push(Span::styled(
            format!(" · {}", format_call_duration(duration_ms)),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }
    line
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
        let header = append_duration_suffix(
            display.format_header_line_with_result(&parsed, result_payload, workspace_root),
            result_payload,
        );
        let details = match display.render_policy().details {
            DetailsPolicy::Expanded => display.format_details(&parsed),
            DetailsPolicy::Hidden => vec![],
        };
        return (header, details);
    }

    let truncated = truncate_json(raw_json);
    let header = Line::from(vec![
        Span::raw("● "),
        Span::styled(
            tool_display_name(name).to_string(),
            Style::default().fg(theme::ACCENT_BRIGHT),
        ),
    ]);
    (
        append_duration_suffix(header, result_payload),
        vec![truncated],
    )
}
