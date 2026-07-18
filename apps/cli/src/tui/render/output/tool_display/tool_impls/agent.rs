use crate::tui::render::output_area::INDENT;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::truncate_ellipsis;
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::build_header_line;
use ratatui::text::Line;
use sdk::tool_input::AgentInput;
use std::path::Path;

/// Deserialize a typed Input from a raw `serde_json::Value`, tolerating
/// missing / malformed fields via `Default`.
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
        let args = parse_input::<AgentInput>(input);
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
        let args = parse_input::<AgentInput>(input);
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
        let args = parse_input::<AgentInput>(input);
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
mod tests {
    use super::*;
    use crate::tui::render::output::tool_display::ToolDisplay;
    use serde_json::json;

    /// issue #646：可指定 role/model 的 agent header helper
    fn agent_header_with_meta(
        description: &str,
        role: Option<&str>,
        model: Option<&str>,
    ) -> String {
        let display = AgentDisplay;
        let mut input = serde_json::Map::new();
        input.insert("description".to_string(), json!(description));
        if let Some(r) = role {
            input.insert("role".to_string(), json!(r));
        }
        if let Some(m) = model {
            input.insert("model".to_string(), json!(m));
        }
        let input = serde_json::Value::Object(input);
        let line = display.format_header_line_with_result(&input, None, None);
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // ── issue #646: AgentDisplay header role/model 4 case ──

    #[test]
    fn agent_header_without_role_or_model_shows_no_meta() {
        let text = agent_header_with_meta("do something", None, None);
        assert_eq!(text, "Agent do something");
    }

    #[test]
    fn agent_header_with_role_only_shows_role() {
        let text = agent_header_with_meta("do something", Some("coder"), None);
        assert_eq!(text, "Agent do something [role: coder]");
    }

    #[test]
    fn agent_header_with_model_only_shows_model() {
        let text = agent_header_with_meta("do something", None, Some("Zhipu/glm-5.2"));
        assert_eq!(text, "Agent do something [model: Zhipu/glm-5.2]");
    }

    #[test]
    fn agent_header_with_role_and_model_shows_both() {
        let text = agent_header_with_meta("do something", Some("coder"), Some("Zhipu/glm-5.2"));
        assert_eq!(
            text,
            "Agent do something [role: coder] [model: Zhipu/glm-5.2]"
        );
    }
}
