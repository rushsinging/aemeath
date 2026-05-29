// 以下模块为后续单源迁移子任务（S2~S5：dialog/input/status）预备的 ViewModel 脚手架，
// 当前尚未接线消费，故按模块标注 dead_code（spinner 派生的 live_status 已接线，不在此列）。
#[allow(dead_code)]
pub mod dialog;
#[allow(dead_code)]
pub mod input;
pub mod live_status;
pub mod output;
#[allow(dead_code)]
pub mod status;
pub mod style;

pub use dialog::{DialogActionViewModel, DialogKind, DialogViewModel};
pub use input::InputAreaViewModel;
pub use live_status::{LiveStatusViewModel, SpinnerLineView};
pub use output::{
    AskUserBlockView, BlockNode, OutputBlockKind, OutputViewModel, TextBlockView,
    ToolCallBlockView, ToolResultBlockView, ToolSemanticStatus,
};
pub use status::{
    StatusContextViewModel, StatusLineViewModel, StatusRuntimeViewModel, StatusSegment,
    StatusSeverity, StatusWorktreeKind,
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
            summary: Some("读取文件".to_string()),
            activity_summary: None,
            result_summary: Some("120 lines".to_string()),
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
