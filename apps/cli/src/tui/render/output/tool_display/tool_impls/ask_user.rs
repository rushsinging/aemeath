use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ToolDisplay, ToolDisplayEntry, ToolRenderPolicy,
};
use super::helpers::build_header_line;
use ratatui::text::Line;
use std::path::Path;

// ── AskUserQuestion ──────────────────────────────────────────────

struct AskUserQuestionDisplay;
impl ToolDisplay for AskUserQuestionDisplay {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn format_header(&self, _input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        // Issue #545: 不再把 question 截断拼进 header，避免长问题信息丢失。
        // 完整 question 由交互区域（blocks/ask_user.rs）按段落渲染。
        self.display_name().to_string()
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
mod tests {
    use super::*;
    use crate::tui::render::output::tool_display::ToolDisplay;
    use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
    use serde_json::json;

    // ── AskUserQuestionDisplay::format_header 回归测试 (#545) ──

    /// Issue #545: header 不应包含 question 截断预览
    #[test]
    fn ask_user_header_never_includes_question_preview() {
        let display = AskUserQuestionDisplay;
        let long_question =
        "这是一个非常非常非常长的问题，包含很多很多很多很多很多很多很多很多很多很多很多很多很多很多细节。";
        let input =
            json!({ "question": long_question, "options": [{"title": "a"}, {"title": "b"}] });
        let header = display.format_header(&input, None);
        assert_eq!(
            header, "Ask",
            "header 应只显示 display_name，不包含 question 截断预览"
        );
    }

    /// Issue #545: format_header_line_with_result 也不应将 question 当 path 截断
    #[test]
    fn ask_user_header_line_never_includes_question_preview() {
        let display = AskUserQuestionDisplay;
        let long_question =
        "这是一个非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常长的多段落问题。";
        let input =
            json!({ "question": long_question, "options": [{"title": "a"}, {"title": "b"}] });
        let result = serde_json::json!({
            "options": [{"title": "a", "description": ""}, {"title": "b", "description": ""}],
            "free_input": null,
        });
        let payload = ToolResultPayload::new(String::new(), result, false, 0);
        let line = display.format_header_line_with_result(&input, Some(&payload), None);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains("非常"),
            "header line 不应包含 question 内容: {text}"
        );
        assert!(
            text.starts_with("Ask"),
            "header line 应以 display_name 开头: {text}"
        );
    }
}
