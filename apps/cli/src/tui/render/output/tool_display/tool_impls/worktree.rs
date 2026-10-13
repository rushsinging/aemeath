use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::typed_data;
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::{build_header_line, format_goal_header};
use ratatui::text::Line;
use std::path::Path;

// ── EnterWorktree ────────────────────────────────────────────────

struct EnterWorktreeDisplay;

fn input_target(input: &serde_json::Value) -> Option<String> {
    ["branch", "path"].into_iter().find_map(|key| {
        input
            .get(key)
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("{key}={value}"))
    })
}

impl ToolDisplay for EnterWorktreeDisplay {
    fn name(&self) -> &str {
        "EnterWorktree"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let goal = input
            .get("goal")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let fallback = input_target(input).unwrap_or_default();
        format_goal_header(self.display_name(), goal, &fallback)
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        match input_target(input) {
            Some(target) => vec![target],
            None => vec![],
        }
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let result: Option<sdk::tool_result::EnterWorktreeResult> = result_payload
            .filter(|payload| !payload.is_error)
            .and_then(|payload| typed_data(Some(payload)));
        let (arg, path_suffix) = match result {
            Some(result) => (
                format!("branch={}", result.branch),
                format!(" ({})", result.workspace_root.display()),
            ),
            None => (input_target(input).unwrap_or_default(), String::new()),
        };
        build_header_line(self.display_name(), &arg, &path_suffix)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Expanded,
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
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let goal = input
            .get("goal")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        format_goal_header(self.display_name(), goal, "")
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
