use super::super::common::{truncate_ellipsis, typed_data};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sdk::tool_input::BashInput;
use std::path::Path;

/// Deserialize a typed Input from a raw `serde_json::Value`, tolerating
/// missing / malformed fields via `Default`.
fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

// ── Bash ─────────────────────────────────────────────────────────

struct BashDisplay;
impl ToolDisplay for BashDisplay {
    fn name(&self) -> &str {
        "Bash"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<BashInput>(input);
        if !args.goal.is_empty() {
            format!("{} {}", self.display_name(), args.goal)
        } else if !args.command.is_empty() {
            // fallback：老 session 缺 goal 时显示截断的 command
            format!(
                "{} {}",
                self.display_name(),
                truncate_ellipsis(&args.command, 80)
            )
        } else {
            self.display_name().to_string()
        }
    }
    fn header_for_subagent(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> String {
        self.format_header(input, workspace_root)
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let args = parse_input::<BashInput>(input);
        if args.command.is_empty() {
            Vec::new()
        } else {
            // 命令全文不截断，由渲染层 Word wrap 自动换行
            vec![args.command]
        }
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Expanded,
            result: ResultPolicy::Visible {
                max_lines: Some(super::super::constants::TOOL_RESULT_TAIL_LINES),
                render_kind: ResultRender::Plain,
                tail_mode: true, // 只显示最后 N 行（与 BackgroundProcessLogs 同源）
            },
        }
    }
    /// 当 result 到达后，从 `BashResult.exit_code` / `signal` 读取
    /// exit code 显示后缀：0/None 空；signal 有值 `(signal N)`；> 0 `(exit N)`。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let args = parse_input::<BashInput>(input);
        let result: Option<sdk::tool_result::BashResult> = typed_data(result_payload);
        let suffix = match result {
            Some(r) if r.exit_code != 0 => {
                if let Some(sig) = r.signal {
                    format!(" (signal {sig})")
                } else {
                    format!(" (exit {})", r.exit_code)
                }
            }
            _ => String::new(),
        };
        let header_text = if !args.goal.is_empty() {
            args.goal
        } else if !args.command.is_empty() {
            // fallback：老 session 缺 goal 时显示截断的 command
            truncate_ellipsis(&args.command, 80)
        } else {
            String::new()
        };
        if header_text.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else if header_text.is_empty() {
            Line::from(vec![
                Span::styled(
                    self.display_name().to_string(),
                    Style::default().fg(theme::ACCENT_BRIGHT),
                ),
                Span::styled(suffix, Style::default().fg(theme::TEXT_MUTED)),
            ])
        } else {
            let mut spans = vec![
                Span::styled(
                    self.display_name().to_string(),
                    Style::default().fg(theme::ACCENT_BRIGHT),
                ),
                Span::raw(format!(" {header_text}")),
            ];
            if !suffix.is_empty() {
                spans.push(Span::styled(suffix, Style::default().fg(theme::TEXT_MUTED)));
            }
            Line::from(spans)
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Bash",
    display: || Box::new(BashDisplay)
});

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
