//! block 嵌套合法性规则。见 spec §4。
use crate::tui::view_model::output::OutputBlockKind;

/// 最大嵌套深度：top(0) → tool_call(1) → result-content(2)。深度从 0 计，最深合法子层级为 2。
pub const MAX_BLOCK_DEPTH: usize = 3;

/// 仅 ToolCall 可含子（ToolResult 结果子块，或 AssistantMessage 文本 / Diagnostic / SystemNotice）；其余为叶子。
pub fn allowed_child(parent: &OutputBlockKind, child: &OutputBlockKind) -> bool {
    matches!(parent, OutputBlockKind::ToolCall(_))
        && matches!(
            child,
            OutputBlockKind::ToolResult(_)
                | OutputBlockKind::AssistantMessage(_)
                | OutputBlockKind::DiagnosticNotice(_)
                | OutputBlockKind::SystemNotice(_)
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::{
        TextBlockView, ToolCallBlockView, ToolResultBlockView, ToolSemanticStatus,
    };
    use crate::tui::view_model::style::SemanticStyle;

    fn tool_result() -> OutputBlockKind {
        OutputBlockKind::ToolResult(ToolResultBlockView {
            key: "t-result".into(),
            tool_title: "Grep".into(),
            summary: None,
            result_text: "done".into(),
        })
    }

    fn tool() -> OutputBlockKind {
        OutputBlockKind::ToolCall(ToolCallBlockView {
            key: "t".into(),
            chat_id: None,
            turn_id: None,
            tool_call_id: None,
            title: "Grep".into(),
            icon: "●".into(),
            semantic_status: ToolSemanticStatus::Success,
            style: SemanticStyle::Running,
            args_preview: None,
            summary: None,
            activity_summary: None,
            result_summary: None,
            collapsible: false,
            collapsed: false,
        })
    }

    fn text(kind: fn(TextBlockView) -> OutputBlockKind) -> OutputBlockKind {
        kind(TextBlockView {
            key: "k".into(),
            text: "x".into(),
            style: SemanticStyle::Muted,
        })
    }

    #[test]
    fn test_allowed_child_tool_allows_assistant_message() {
        let parent = tool();
        let child = text(OutputBlockKind::AssistantMessage);
        assert!(allowed_child(&parent, &child));
    }

    #[test]
    fn test_allowed_child_tool_allows_tool_result() {
        // ToolCall → ToolResult 合法（结果升为子块，#60）。
        assert!(allowed_child(&tool(), &tool_result()));
    }

    #[test]
    fn test_allowed_child_tool_result_is_leaf() {
        // ToolResult 为叶子，不接受任何子（含 ToolResult 自身）。
        let child = text(OutputBlockKind::AssistantMessage);
        assert!(!allowed_child(&tool_result(), &child));
        assert!(!allowed_child(&tool_result(), &tool_result()));
    }

    #[test]
    fn test_allowed_child_tool_allows_diagnostic_and_system_notice() {
        let parent = tool();
        assert!(allowed_child(
            &parent,
            &text(OutputBlockKind::DiagnosticNotice)
        ));
        assert!(allowed_child(&parent, &text(OutputBlockKind::SystemNotice)));
    }

    #[test]
    fn test_allowed_child_tool_rejects_tool_and_user_message() {
        let parent = tool();
        // ToolCall 子（禁止再嵌套 tool_call）。
        assert!(!allowed_child(&parent, &tool()));
        // UserMessage 不是合法的 result 富渲染子块。
        assert!(!allowed_child(&parent, &text(OutputBlockKind::UserMessage)));
    }

    #[test]
    fn test_allowed_child_non_tool_parent_is_leaf() {
        // 非 ToolCall 父均为叶子，不接受任何子。
        let child = text(OutputBlockKind::AssistantMessage);
        assert!(!allowed_child(
            &text(OutputBlockKind::AssistantMessage),
            &child
        ));
        assert!(!allowed_child(&text(OutputBlockKind::UserMessage), &child));
        assert!(!allowed_child(&OutputBlockKind::Separator, &child));
    }

    #[test]
    fn test_max_block_depth_value() {
        assert_eq!(MAX_BLOCK_DEPTH, 3);
    }
}
