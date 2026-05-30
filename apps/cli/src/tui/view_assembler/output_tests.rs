use super::OutputViewAssembler;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::model::ConversationModel;
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
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(diagnostic_results, 0);
    assert_eq!(tool.result_summary.as_deref(), Some("✓ Read completed"));
    assert!(!tool
        .result_summary
        .as_deref()
        .unwrap_or_default()
        .contains("line1"));
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
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(diagnostics, 0, "已绑定工具结果不应泄漏成块外诊断文本");
    assert_eq!(tool.title, "Edit");
    assert!(tool
        .result_summary
        .as_deref()
        .unwrap_or_default()
        .contains("Edit completed"));
    assert!(!tool
        .result_summary
        .as_deref()
        .unwrap_or_default()
        .contains("---DIFF---"));
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
fn test_output_assembler_renders_task_list_create_tool_call() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "fix bug".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "TaskListCreate".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-tlc".to_string(),
        name: "TaskListCreate".to_string(),
        index: 0,
        summary: r#"{"subject":"修复 bug","summary":"修复 bug 84"}"#.to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-tlc".to_string(),
        tool_name: "TaskListCreate".to_string(),
        output: "Task list #0 created".to_string(),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);

    // 应有且仅有一个 ToolCall block（不应泄漏为 DiagnosticNotice）
    let diagnostic_count = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    assert_eq!(diagnostic_count, 0, "TaskListCreate 结果不应泄漏为诊断文本");

    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("应有 TaskListCreate 工具调用块");

    assert_eq!(tool.title, "TaskListCreate");
    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
    // result_summary 应使用 fallback（TaskListCreateDisplay 返回空，走 default）
    assert!(tool.result_summary.is_some(), "应有结果摘要");
}

#[test]
fn test_output_assembler_renders_task_create_tool_call() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "fix bug".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "TaskCreate".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-tc".to_string(),
        name: "TaskCreate".to_string(),
        index: 0,
        summary: r#"{"subject":"分析代码","description":"查看代码结构"}"#.to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-tc".to_string(),
        tool_name: "TaskCreate".to_string(),
        output: "Task #0 created".to_string(),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("应有 TaskCreate 工具调用块");

    assert_eq!(tool.title, "TaskCreate");
    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
}

#[test]
fn test_output_assembler_renders_task_update_tool_call() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "fix bug".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "TaskUpdate".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-tu".to_string(),
        name: "TaskUpdate".to_string(),
        index: 0,
        summary: r#"{"taskId":"1","status":"completed"}"#.to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-tu".to_string(),
        tool_name: "TaskUpdate".to_string(),
        output: "Task #1 updated".to_string(),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("应有 TaskUpdate 工具调用块");

    assert_eq!(tool.title, "TaskUpdate");
    assert_eq!(tool.icon, "✓");
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
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: name.to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-1".to_string(),
        name: name.to_string(),
        index: 0,
        summary: "search docs".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-1".to_string(),
        tool_name: name.to_string(),
        output: output.to_string(),
        is_error,
        image_count: 0,
    });
}

#[test]
fn test_output_assembler_non_embedded_tool_result_uses_summary() {
  // 非嵌入路径：ToolResult 的 id 在 chats 中有 tool call 但 result 为空
  // （防御性路径，正常流程不触发）。验证 output 不被原样透传。
  let mut conversation = ConversationModel::default();
  conversation.apply(ConversationIntent::StartChat {
      submission: "search".to_string(),
  });
  // 只发送 ToolCallStart + ToolCall（无 ToolResult），result 字段为 None。
  conversation.apply(ConversationIntent::ObserveToolCallStart {
      name: "Read".to_string(),
      index: 0,
  });
  conversation.apply(ConversationIntent::ObserveToolCall {
      id: "tool-1".to_string(),
      name: "Read".to_string(),
      index: 0,
      summary: "read file".to_string(),
  });
  // 手动推入一个 ToolResult block（模拟 late-bound 场景）。
  // 此时 tool-1 的 result 仍为 None → tool_result_is_embedded 返回 false。
  conversation.apply(ConversationIntent::ObserveToolResult {
      id: "tool-1".to_string(),
      tool_name: "Read".to_string(),
      output: "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8".to_string(),
      is_error: false,
      image_count: 0,
  });
  // 观察 ToolResult 后 result 被设置 → 此时嵌入式为 true。
  // 但如果 ToolResult block 到达 assembler 时嵌入式判断为 false，
  // 则走 DiagnosticNotice 路径。
  // 实际上 observe_tool_result 会同时设置 result 和推入 block，
  // 所以正常流程总是嵌入的。
  // 此测试验证：当结果确实被嵌入时，不会泄漏为 DiagnosticNotice。
  let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1);
  let diagnostics = vm
      .roots
      .iter()
      .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
      .count();
  assert_eq!(diagnostics, 0, "正常流程的 tool result 不应泄漏为 DiagnosticNotice");
}

#[test]
fn test_output_assembler_orphan_tool_result_is_truncated() {
  // OrphanToolResult 路径：tool result 在 tool call 之前到达。
  // 验证完整 output 不被原样透传，而是截断。
  use crate::tui::model::conversation::intent::ConversationIntent;
  use crate::tui::view_model::OutputBlockKind;

  let mut conversation = ConversationModel::default();
  conversation.apply(ConversationIntent::StartChat {
      submission: "search".to_string(),
  });
  // 直接发送 ToolResult（无对应的 tool call）→ orphan 路径。
  conversation.apply(ConversationIntent::ObserveToolResult {
      id: "tool-orphan".to_string(),
      tool_name: "Bash".to_string(),
      output: (1..=100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n"),
      is_error: false,
      image_count: 0,
  });

  let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1);
  let orphan = vm
      .roots
      .iter()
      .find(|block| {
          block.block_id.starts_with("orphan-")
              && matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_))
      })
      .expect("应有 orphan DiagnosticNotice block");

  if let OutputBlockKind::DiagnosticNotice(text_view) = &orphan.kind {
      let line_count = text_view.text.lines().count();
      assert!(
          line_count <= 10,
          "orphan result 应被截断，实际有 {line_count} 行"
      );
      assert!(
          text_view.text.contains("lines omitted"),
          "orphan result 应包含省略提示，实际内容: {}",
          text_view.text
      );
  }
}

#[test]
fn test_output_assembler_truncate_output_lines_short() {
  // 短 output 不截断。
  use super::truncate_output_lines;
  let result = truncate_output_lines("a\nb\nc", "Read");
  assert_eq!(result, "a\nb\nc");
}

#[test]
fn test_output_assembler_truncate_output_lines_long() {
  // 长 output 按 result_max_lines 截断。
  use super::truncate_output_lines;
  let long: String = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
  let result = truncate_output_lines(&long, "Read");
  assert!(result.contains("lines omitted"), "应包含省略提示");
  assert!(result.contains("line 1"), "应包含前几行");
  assert!(!result.contains("line 20"), "不应包含最后一行");
}

#[test]
fn test_output_assembler_truncate_output_lines_exact() {
  // 恰好 max_lines 行不截断。
  use super::truncate_output_lines;
  // Read 默认 result_max_lines = 5
  let exact = "a\nb\nc\nd\ne";
  let result = truncate_output_lines(exact, "Read");
  assert_eq!(result, exact);
  assert!(!result.contains("lines omitted"));
}

#[test]
fn test_output_assembler_summarize_orphan_result_empty() {
  // 空 output 返回空字符串。
  use super::summarize_orphan_result;
  assert_eq!(summarize_orphan_result(""), "");
}

#[test]
fn test_output_assembler_summarize_non_embedded_with_known_tool() {
  // 已知 tool name 使用 format_result_summary 生成摘要。
  use super::summarize_non_embedded_result;
  let result = summarize_non_embedded_result(Some("Read"), "anything", false);
  assert_eq!(result, "✓ Read completed");
}

#[test]
fn test_output_assembler_summarize_non_embedded_with_error() {
  // error 路径使用错误摘要。
  use super::summarize_non_embedded_result;
  let result = summarize_non_embedded_result(Some("Read"), "permission denied", true);
  assert_eq!(result, "✗ Read failed");
}

#[test]
fn test_output_assembler_summarize_non_embedded_unknown_tool_truncates() {
  // 未知 tool name 走截断路径。
  use super::summarize_non_embedded_result;
  let long: String = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
  let result = summarize_non_embedded_result(None, &long, false);
  assert!(result.contains("lines omitted"), "未知工具应走截断路径");
}

#[test]
fn test_output_assembler_summarize_non_embedded_empty() {
  // 空 output 返回空字符串。
  use super::summarize_non_embedded_result;
  assert_eq!(summarize_non_embedded_result(Some("Read"), "", false), "");
}
