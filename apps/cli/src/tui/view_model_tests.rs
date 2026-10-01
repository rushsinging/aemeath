use super::output::{
    BlockNode, OutputBlockKind, OutputViewModel, ToolCallBlockView, ToolSemanticStatus,
};
use super::style::SemanticStyle;

#[test]
fn test_output_view_model_accepts_tool_block() {
    let kind = OutputBlockKind::ToolCall(ToolCallBlockView {
        key: "chat-1/turn-1/tool-1".to_string(),
        chat_id: Some("chat-1".to_string()),
        run_id: Some("turn-1".to_string()),
        tool_call_id: Some("tool-1".to_string()),
        title: "Read(src/main.rs)".to_string(),
        icon: "✓".to_string(),
        semantic_status: ToolSemanticStatus::Success,
        style: SemanticStyle::Success,
        args_preview: Some("src/main.rs".to_string()),
        streaming_preview: None,
        result_summary: None,
        result_payload: None,
        workspace_root: None,
        collapsible: true,
        collapsed: false,
        agent_meta: None,
    });
    let node = BlockNode {
        block_id: "chat-1/turn-1/tool-1".to_string(),
        block_version: kind.cache_version(),
        kind,
        children: Vec::new(),
    };
    let model = OutputViewModel {
        roots: vec![node.into()],
        version: 1,
        source_total_lines: None,
        folded_earlier_lines: 0,
    };
    assert_eq!(model.roots.len(), 1);
}
