use crate::tui::render::output_area::INDENT;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::{str_arg, truncate_ellipsis, typed_data};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::build_header_line;
use ratatui::text::Line;
use std::path::Path;

// ── Agent ────────────────────────────────────────────────────────

struct AgentDisplay;
impl ToolDisplay for AgentDisplay {
    fn name(&self) -> &str {
        "Agent"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let desc = str_arg(input, "description", "sub-task");
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
        let prompt = str_arg(input, "prompt", "");
        if prompt.is_empty() {
            return vec![];
        }
        vec![truncate_ellipsis(
            prompt,
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
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let target = typed_data::<sdk::tool_result::AgentResult>(result_payload)
            .and_then(|r| r.task_id)
            .filter(|id| !id.is_empty());
        let arg = match target {
            Some(id) => format!("{description} -> [{id}]"),
            None => description.to_string(),
        };
        // issue #499：追加 role/model 标记（由 merge_agent_meta 从 agent_meta 合并而来）
        let mut suffix = String::new();
        if let Some(role) = input.get("role").and_then(|v| v.as_str()) {
            suffix.push_str(&format!(" [role: {role}]"));
        }
        if let Some(model) = input.get("model").and_then(|v| v.as_str()) {
            suffix.push_str(&format!(" [model: {model}]"));
        }
        build_header_line(self.display_name(), &arg, &suffix)
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
    use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
    use serde_json::json;

    // ── AgentDisplay::format_header_line_with_result 回归测试 (#422) ──

    fn agent_header_text(description: &str, result_content: Option<serde_json::Value>) -> String {
        let display = AgentDisplay;
        let input = json!({ "description": description });
        let payload = result_content.map(|c| ToolResultPayload::new(String::new(), c, false, 0));
        let line = display.format_header_line_with_result(&input, payload.as_ref(), None);
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

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

    /// 有 task_id 时应显示 `-> [id]` 后缀
    #[test]
    fn agent_header_with_task_id_shows_suffix() {
        let text = agent_header_text(
            "do something",
            Some(json!({ "task_id": "task-42", "output": "done" })),
        );
        assert_eq!(text, "Agent do something -> [task-42]");
    }

    /// task_id 为 None 时不应显示 `-> []` 后缀（#422 回归）
    #[test]
    fn agent_header_without_task_id_hides_suffix() {
        let text = agent_header_text(
            "do something",
            Some(json!({ "task_id": null, "output": "done" })),
        );
        assert_eq!(text, "Agent do something");
    }

    /// task_id 为空字符串时也不应显示后缀
    #[test]
    fn agent_header_with_empty_task_id_hides_suffix() {
        let text = agent_header_text(
            "do something",
            Some(json!({ "task_id": "", "output": "done" })),
        );
        assert_eq!(text, "Agent do something");
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
