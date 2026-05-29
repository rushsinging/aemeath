#![allow(dead_code)]

pub mod dialog;
pub mod input;
pub mod output;
pub mod status;
pub mod style;

pub use dialog::{DialogActionViewModel, DialogKind, DialogViewModel};
pub use input::InputAreaViewModel;
pub use output::{
    AskUserBlockView, BlockNode, OutputBlockKind, OutputBlockView, OutputViewModel, TextBlockView,
    ToolCallBlockView, ToolSemanticStatus,
};
pub use status::{StatusLineViewModel, StatusSegment, StatusSeverity};
pub use style::SemanticStyle;

#[cfg(test)]
mod tests {
    use super::output::{
        OutputBlockKind, OutputBlockView, OutputViewModel, ToolCallBlockView, ToolSemanticStatus,
    };
    use super::style::SemanticStyle;

    #[test]
    fn test_output_view_model_accepts_tool_block() {
        let block = OutputBlockView {
            block_id: "chat-1/turn-1/tool-1".to_string(),
            block_version: 1,
            kind: OutputBlockKind::ToolCall(ToolCallBlockView {
                key: "chat-1/turn-1/tool-1".to_string(),
                chat_id: Some("chat-1".to_string()),
                turn_id: Some("turn-1".to_string()),
                tool_call_id: Some("tool-1".to_string()),
                title: "Read(src/main.rs)".to_string(),
                icon: "✓".to_string(),
                semantic_status: ToolSemanticStatus::Success,
                style: SemanticStyle::Success,
                args_preview: Some("src/main.rs".to_string()),
                summary: Some("读取文件".to_string()),
                activity_summary: None,
                result_summary: Some("120 lines".to_string()),
                collapsible: true,
                collapsed: false,
            }),
        };
        let model = OutputViewModel {
            blocks: vec![block],
            roots: Vec::new(),
            version: 1,
            follow_tail_hint: true,
        };
        assert_eq!(model.blocks.len(), 1);
    }
}
