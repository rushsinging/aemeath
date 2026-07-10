use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::typed_data;
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::build_header_line;
use ratatui::text::Line;
use std::path::Path;

// ── EnterWorktree ────────────────────────────────────────────────

struct EnterWorktreeDisplay;
impl ToolDisplay for EnterWorktreeDisplay {
    fn name(&self) -> &str {
        "EnterWorktree"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let target = input
            .get("branch")
            .and_then(|branch| branch.as_str())
            .or_else(|| input.get("path").and_then(|path| path.as_str()))
            .unwrap_or("worktree");
        format!("{} {target}", self.display_name())
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_header_line_with_result(
        &self,
        _input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let result: Option<sdk::tool_result::EnterWorktreeResult> = typed_data(result_payload);
        let branch = result
            .as_ref()
            .map(|r| r.branch.clone())
            .unwrap_or_else(|| "(default)".to_string());
        let arg = format!("branch={branch}");
        let path_suffix = result
            .map(|r| format!(" ({})", r.workspace_root.display()))
            .unwrap_or_default();
        build_header_line(self.display_name(), &arg, &path_suffix)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(16),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "EnterWorktree",
    display: || Box::new(EnterWorktreeDisplay)
});

// ── ExitWorktree ─────────────────────────────────────────────────

struct ExitWorktreeDisplay;
impl ToolDisplay for ExitWorktreeDisplay {
    fn name(&self) -> &str {
        "ExitWorktree"
    }
    fn format_header(&self, _input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        self.display_name().to_string()
    }
    fn format_header_line_with_result(
        &self,
        _input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let path_suffix = typed_data::<sdk::tool_result::ExitWorktreeResult>(result_payload)
            .map(|r| format!(" (back to {})", r.workspace_root.display()))
            .unwrap_or_default();
        build_header_line(self.display_name(), "", &path_suffix)
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(16),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "ExitWorktree",
    display: || Box::new(ExitWorktreeDisplay)
});
