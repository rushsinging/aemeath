pub mod dialog;
pub mod input;
pub mod live_status;
pub mod nesting;
pub mod output;
pub mod status;
pub mod style;
pub mod tool_name;

pub use dialog::{DialogActionViewModel, DialogKind, DialogViewModel};
pub use input::InputAreaViewModel;
pub use live_status::{LiveStatusViewModel, SpinnerLineView};
pub use nesting::{allowed_child, MAX_BLOCK_DEPTH};
pub use output::{
    AskUserBatchBlockView, BlockNode, HookNoticeBlockView, HookNoticeSemanticKind, OutputBlockKind,
    OutputViewModel, TextBlockView, ToolCallBlockView, ToolResultBlockView, ToolSemanticStatus,
};
pub use status::{
    StatusContextViewModel, StatusLineViewModel, StatusNoticeViewKind, StatusNoticeViewModel,
    StatusRuntimeViewModel, StatusSegment, StatusSeverity, StatusViewModel, StatusWorktreeKind,
};
pub use style::SemanticStyle;

#[cfg(test)]
mod tests {
    use super::output::{
        BlockNode, OutputBlockKind, OutputViewModel, ToolCallBlockView, ToolSemanticStatus,
    };
    use super::style::SemanticStyle;

    #[test]
    fn test_output_view_model_accepts_tool_block() {
        let kind = OutputBlockKind::ToolCall(ToolCallBlockView {
            key: "chat-1/turn-1/tool-1".to_string(),
            chat_id: Some("chat-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            tool_call_id: Some("tool-1".to_string()),
            title: "Read(src/main.rs)".to_string(),
            icon: "✓".to_string(),
            semantic_status: ToolSemanticStatus::Success,
            style: SemanticStyle::Success,
            args_preview: Some("src/main.rs".to_string()),
            activity_summary: None,
            result_summary: None,
            collapsible: true,
            collapsed: false,
        });
        let node = BlockNode {
            block_id: "chat-1/turn-1/tool-1".to_string(),
            block_version: kind.cache_version(),
            kind,
            children: Vec::new(),
        };
        let model = OutputViewModel {
            roots: vec![node],
            version: 1,
            follow_tail_hint: true,
        };
        assert_eq!(model.roots.len(), 1);
    }
}
