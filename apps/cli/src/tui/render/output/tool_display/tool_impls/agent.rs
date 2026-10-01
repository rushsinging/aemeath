use crate::tui::render::output_area::INDENT;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::truncate_ellipsis;
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::build_header_line;
use ratatui::text::Line;
use serde::Deserialize;
use std::path::Path;

#[derive(Default, Deserialize)]
#[serde(default)]
struct AgentDisplayInput {
    prompt: String,
    description: String,
}

/// Deserialize the display-only projection, intentionally tolerating legacy
/// calls that predate the required execution `role` field.
fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

// ── Agent ────────────────────────────────────────────────────────

struct AgentDisplay;
impl ToolDisplay for AgentDisplay {
    fn name(&self) -> &str {
        "Agent"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<AgentDisplayInput>(input);
        let desc = if args.description.is_empty() {
            "sub-task"
        } else {
            &args.description
        };
        let role = input.get("role").and_then(|role| role.as_str());
        let model = input.get("model").and_then(|model| model.as_str());
        let mut header = format!("{} {desc}", self.display_name());
        if let Some(r) = role {
            header.push_str(&format!(" [role: {r}]"));
        }
        if let Some(m) = model {
            header.push_str(&format!(" [model: {m}]"));
        }
        header
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let args = parse_input::<AgentDisplayInput>(input);
        if args.prompt.is_empty() {
            return vec![];
        }
        vec![truncate_ellipsis(
            &args.prompt,
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
                tail_mode: false,
            },
        }
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        _result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let args = parse_input::<AgentDisplayInput>(input);
        let description = args.description.as_str();
        // issue #499：追加 role/model 标记（由 merge_agent_meta 从 agent_meta 合并而来）
        let mut suffix = String::new();
        if let Some(role) = input.get("role").and_then(|v| v.as_str()) {
            suffix.push_str(&format!(" [role: {role}]"));
        }
        if let Some(model) = input.get("model").and_then(|v| v.as_str()) {
            suffix.push_str(&format!(" [model: {model}]"));
        }
        build_header_line(self.display_name(), description, &suffix)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Agent",
    display: || Box::new(AgentDisplay)
});

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
