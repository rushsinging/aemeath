use super::OutputViewAssembler;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::render::output::rendered::RenderCtx;
use crate::tui::view_model::{OutputBlockKind, ToolSemanticStatus};

#[test]
fn test_output_assembler_maps_tool_status_to_icon() {
    let mut conversation = ConversationModel::default();
    add_completed_tool_after_thinking(&mut conversation, "Read", "ok");

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
}

#[test]
fn test_output_assembler_keeps_tool_result_inside_tool_after_thinking() {
    let mut conversation = ConversationModel::default();
    add_completed_tool_after_thinking(
        &mut conversation,
        "Grep",
        "/tmp/docs/bug/active.md:18:match",
    );

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let diagnostic_results = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(diagnostic_results, 0);
    assert_eq!(tool.title, "Grep");
    assert_eq!(tool.result_summary.as_deref(), Some("✓ Grep completed"));
}

#[test]
fn test_output_assembler_summarizes_embedded_tool_result_without_full_output() {
    let mut conversation = ConversationModel::default();
    let full_output = "line1\nline2\nline3\nline4\nline5\nline6";
    add_completed_tool_after_thinking(&mut conversation, "Read", full_output);

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let diagnostic_results = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    let tool_node = vm
        .roots
        .iter()
        .find(|block| matches!(&block.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool block");
    let OutputBlockKind::ToolCall(tool) = &tool_node.kind else {
        panic!("expected tool call");
    };

    assert_eq!(diagnostic_results, 0);
    assert_eq!(tool.result_summary.as_deref(), Some("✓ Read completed"));
    assert_eq!(tool_node.children.len(), 1);
    let OutputBlockKind::ToolResult(result) = &tool_node.children[0].kind else {
        panic!("expected tool result child");
    };
    assert_eq!(result.result_text, "✓ Read completed");
    assert!(!result.result_text.contains("line1"));
}

#[test]
fn test_output_assembler_keeps_assistant_text_outside_read_result() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "查看 active bug".to_string(),
    });
    add_completed_tool(
        &mut conversation,
        "tool-read",
        "Read",
        r#"{"file_path":"docs/bug/active.md"}"#,
        "## 活跃 Bug（21 个）\n\n # │ 标题 │ 优先级 │ 状态\n|---|------|--------|------|",
        false,
    );
    conversation.apply(ConversationIntent::ObserveAssistantText {
        text: "我看到 active bug 列表，下面是分析。".to_string(),
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool_node = vm
        .roots
        .iter()
        .find(|block| matches!(&block.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool block");
    let assistant = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::AssistantMessage(text) => Some(text),
            _ => None,
        })
        .expect("assistant text block");

    let OutputBlockKind::ToolCall(tool) = &tool_node.kind else {
        panic!("expected tool call");
    };
    assert_eq!(tool.result_summary.as_deref(), Some("✓ Read completed"));
    let OutputBlockKind::ToolResult(result) = &tool_node.children[0].kind else {
        panic!("expected tool result child");
    };
    assert_eq!(result.result_text, "✓ Read completed");
    assert!(!result.result_text.contains("## 活跃 Bug"));
    assert_eq!(assistant.text, "我看到 active bug 列表，下面是分析。");
}

#[test]
fn test_output_assembler_late_bound_tool_result_stays_inside_tool_block() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "edit docs".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "Edit".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-1".to_string(),
        tool_name: "Edit".to_string(),
        output: "replaced 1 occurrence(s) in docs/bug/active.md\n---DIFF---\nold\n---DIFF---\nnew"
            .to_string(),
        is_error: false,
        image_count: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-1".to_string(),
        name: "Edit".to_string(),
        index: 0,
        summary: r#"{"file_path":"docs/bug/active.md"}"#.to_string(),
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let diagnostics = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    let tool_root = vm
        .roots
        .iter()
        .find(|block| matches!(&block.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool block");

    assert_eq!(diagnostics, 0, "已绑定工具结果不应泄漏成块外诊断文本");

    // 嵌入式 Edit ToolResult 子块应渲染为加减色 diff：含 ---DIFF--- 时透传原文，
    // render_tool_result → render_edit_diff 消费标记，输出 old/new diff 行（refs #90）。
    let result_child = tool_root
        .children
        .iter()
        .find(|child| matches!(&child.kind, OutputBlockKind::ToolResult(_)))
        .expect("ToolResult 子块存在");
    let rendered = result_child
        .kind
        .component()
        .render_self(&result_child.block_id, &RenderCtx { width: 80 });
    let plains: Vec<&str> = rendered.lines.iter().map(|l| l.plain.as_str()).collect();

    assert!(
        !plains.iter().any(|p| p.contains("---DIFF---")),
        "diff 渲染后不应残留原始 ---DIFF--- 标记, got: {plains:?}"
    );
    assert!(
        !plains.iter().any(|p| p.contains("Edit completed")),
        "Edit 结果应渲染为 diff 而非 ✓ Edit completed 摘要, got: {plains:?}"
    );
    assert!(
        plains.iter().any(|p| p.contains("- ") && p.contains("old")),
        "应含删除行（- old）, got: {plains:?}"
    );
    assert!(
        plains.iter().any(|p| p.contains("+ ") && p.contains("new")),
        "应含新增行（+ new）, got: {plains:?}"
    );
}

#[test]
fn test_output_assembler_uses_error_summary_for_failed_tool_result() {
    let mut conversation = ConversationModel::default();
    add_failed_tool_after_thinking(&mut conversation, "Read", "permission denied");

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(tool.result_summary.as_deref(), Some("✗ Read failed"));
}

#[test]
fn test_output_assembler_attaches_tool_result_as_child_of_tool_call() {
    // 工具结果升为子块（#60）：完成的 ToolCall root 应带一个 ToolResult 子节点，
    // 子节点 key 为 `<toolid>-result`，且子节点本身为叶子。
    let mut conversation = ConversationModel::default();
    add_completed_tool_after_thinking(&mut conversation, "Read", "ok");

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1);

    let tool_node = vm
        .roots
        .iter()
        .find(|n| matches!(&n.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool call root");
    assert_eq!(tool_node.children.len(), 1, "完成工具应附带 1 个结果子块");
    let result = &tool_node.children[0];
    assert!(
        matches!(&result.kind, OutputBlockKind::ToolResult(_)),
        "子块应为 ToolResult 变体"
    );
    assert_eq!(result.block_id, "tool-1-result");
    assert!(result.children.is_empty(), "ToolResult 为叶子");
    // ToolResult 不应作为顶层 root 出现（必须是 tool_call 的子）。
    assert!(
        !vm.roots
            .iter()
            .any(|n| matches!(&n.kind, OutputBlockKind::ToolResult(_))),
        "ToolResult 不应是顶层 root"
    );
}

#[test]
fn test_output_assembler_pending_tool_has_no_result_child() {
    // 边界：未产出结果（仅 ToolCallStart，无 ObserveToolResult）的工具不附结果子块。
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "search".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "Read".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: "search".to_string(),
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1);

    let tool_node = vm
        .roots
        .iter()
        .find(|n| matches!(&n.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool call root");
    assert!(
        tool_node.children.is_empty(),
        "无结果的工具不应附带结果子块"
    );
}

fn add_failed_tool_after_thinking(conversation: &mut ConversationModel, name: &str, output: &str) {
    add_tool_after_thinking(conversation, name, output, true);
}

fn add_completed_tool_after_thinking(
    conversation: &mut ConversationModel,
    name: &str,
    output: &str,
) {
    add_tool_after_thinking(conversation, name, output, false);
}

fn add_tool_after_thinking(
    conversation: &mut ConversationModel,
    name: &str,
    output: &str,
    is_error: bool,
) {
    conversation.apply(ConversationIntent::StartChat {
        submission: "search".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveThinkingText {
        text: "thinking".to_string(),
    });
    conversation.apply(ConversationIntent::CompleteTextBlock);
    add_completed_tool(conversation, "tool-1", name, "search docs", output, is_error);
}

fn add_completed_tool(
    conversation: &mut ConversationModel,
    id: &str,
    name: &str,
    summary: &str,
    output: &str,
    is_error: bool,
) {
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: name.to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: id.to_string(),
        name: name.to_string(),
        index: 0,
        summary: summary.to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: id.to_string(),
        tool_name: name.to_string(),
        output: output.to_string(),
        is_error,
        image_count: 0,
    });
}
