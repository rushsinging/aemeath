use crate::tui::render::output_area::INDENT;
use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::{str_arg, truncate_ellipsis, typed_data};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::Path;

// ── Bash ─────────────────────────────────────────────────────────

struct BashDisplay;
impl ToolDisplay for BashDisplay {
    fn name(&self) -> &str {
        "Bash"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let cmd = str_arg(input, "command", "");
        // 命令可含任意 UTF-8（如中文 PR 标题），用宽度感知、char 边界安全的截断。
        if cmd.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {}", self.display_name(), truncate_ellipsis(cmd, 80))
        }
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let cmd = str_arg(input, "command", "");
        if cmd.is_empty() {
            return vec![];
        }
        // 截断显示，避免过长命令占用太多空间
        vec![truncate_ellipsis(
            cmd,
            200usize.saturating_sub(INDENT.len()),
        )]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Expanded,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: true, // 只显示最后 5 行
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
        let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
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
        if cmd.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else if cmd.is_empty() {
            Line::from(vec![
                Span::styled(
                    self.display_name().to_string(),
                    Style::default().fg(theme::ACCENT_BRIGHT),
                ),
                Span::styled(suffix, Style::default().fg(theme::TEXT_MUTED)),
            ])
        } else {
            let display_cmd = truncate_ellipsis(cmd, 80);
            let mut spans = vec![
                Span::styled(
                    self.display_name().to_string(),
                    Style::default().fg(theme::ACCENT_BRIGHT),
                ),
                Span::raw(format!(" {display_cmd}")),
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
