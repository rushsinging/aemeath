use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use crate::tui::view_model::tool_name::tool_display_name;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::Path;

use super::policy::ToolRenderPolicy;

/// Trait for customizing how a tool call is displayed in the TUI output area.
pub trait ToolDisplay: Send + Sync {
    /// Tool name as registered in the tool registry.
    fn name(&self) -> &str;

    /// 用户可见的 display name（默认从 `tool_display_name` 映射查表）。
    fn display_name(&self) -> &str {
        tool_display_name(self.name())
    }

    /// Format the header line as plain string.
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String;

    /// Format the header line as styled `Line`。
    fn format_header_line(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let text = self.format_header(input, workspace_root);
        let name = self.display_name().to_string();
        if let Some(rest) = text.strip_prefix(&name) {
            Line::from(vec![
                Span::styled(name, Style::default().fg(theme::ACCENT_BRIGHT)),
                Span::raw(rest.to_string()),
            ])
        } else {
            Line::from(Span::styled(
                text,
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        }
    }

    /// Format the header line with optional structured result payload。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        _result_payload: Option<&ToolResultPayload>,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        self.format_header_line(input, workspace_root)
    }

    /// Format detail lines shown below the header.
    fn format_details(&self, input: &serde_json::Value) -> Vec<String>;

    /// 返回该工具的渲染策略。
    fn render_policy(&self) -> ToolRenderPolicy;
}
