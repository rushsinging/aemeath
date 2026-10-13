use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ToolDisplay, ToolDisplayEntry, ToolRenderPolicy,
};
use super::helpers::{build_header_line, format_goal_header};
use ratatui::text::Line;
use std::path::Path;

// ── AskUserQuestion ──────────────────────────────────────────────

struct AskUserQuestionDisplay;
impl ToolDisplay for AskUserQuestionDisplay {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        // Issue #545: 不再把 question 截断拼进 header；#1964 用 goal 表达意图。
        // 完整 question 由交互区域（blocks/ask_user.rs）按段落渲染。
        let goal = input
            .get("goal")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        format_goal_header(self.display_name(), goal, "")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden, // answer is already echoed via App::append_user_echo
        }
    }
    /// AskUser 的答案已由交互区域展示，header 不重复投影结果内容。
    fn format_header_line_with_result(
        &self,
        _input: &serde_json::Value,
        _result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        build_header_line(self.display_name(), "", "")
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "AskUserQuestion",
    display: || Box::new(AskUserQuestionDisplay)
});

#[cfg(test)]
#[path = "ask_user_tests.rs"]
mod tests;
