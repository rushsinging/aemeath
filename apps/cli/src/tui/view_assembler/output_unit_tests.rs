//! output.rs 辅助函数单元测试 + 非嵌入/orphan 集成测试。

use super::OutputViewAssembler;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::view_model::OutputBlockKind;

#[test]
fn test_non_embedded_tool_result_uses_summary() {
    // 非嵌入路径防御性测试：正常流程中 tool result 总是被嵌入，
    // 此处验证 assembler 不会将嵌入式结果泄漏为 DiagnosticNotice。
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
        summary: "read file".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-1".to_string(),
        tool_name: "Read".to_string(),
        output: "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8".to_string(),
        is_error: false,
        image_count: 0,
    });
    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1);
    let diagnostics = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    assert_eq!(
        diagnostics, 0,
        "正常流程的 tool result 不应泄漏为 DiagnosticNotice"
    );
}

#[test]
fn test_orphan_tool_result_is_truncated() {
    // OrphanToolResult 路径：tool result 在 tool call 之前到达。
    // 验证完整 output 不被原样透传，而是截断。
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "search".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-orphan".to_string(),
        tool_name: "Bash".to_string(),
        output: (1..=100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
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

// ── 辅助函数单元测试 ──────────────────────────────────────────────

#[test]
fn test_truncate_output_lines_short() {
    use super::truncate_output_lines;
    let result = truncate_output_lines("a\nb\nc", "Read");
    assert_eq!(result, "a\nb\nc");
}

#[test]
fn test_truncate_output_lines_long() {
    use super::truncate_output_lines;
    let long: String = (1..=20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let result = truncate_output_lines(&long, "Read");
    assert!(result.contains("lines omitted"), "应包含省略提示");
    assert!(result.contains("line 1"), "应包含前几行");
    assert!(!result.contains("line 20"), "不应包含最后一行");
}

#[test]
fn test_truncate_output_lines_exact() {
    use super::truncate_output_lines;
    let exact = "a\nb\nc\nd\ne";
    let result = truncate_output_lines(exact, "Read");
    assert_eq!(result, exact);
    assert!(!result.contains("lines omitted"));
}

#[test]
fn test_summarize_orphan_result_empty() {
    use super::summarize_orphan_result;
    assert_eq!(summarize_orphan_result(""), "");
}

#[test]
fn test_summarize_orphan_result_long_truncates() {
    use super::summarize_orphan_result;
    let long: String = (1..=20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let result = summarize_orphan_result(&long);
    assert!(result.contains("lines omitted"), "orphan 长文本应被截断");
}

#[test]
fn test_summarize_non_embedded_with_known_tool() {
    use super::summarize_non_embedded_result;
    let result = summarize_non_embedded_result(Some("Read"), "anything", false);
    assert_eq!(result, "✓ Read completed");
}

#[test]
fn test_summarize_non_embedded_with_error() {
    use super::summarize_non_embedded_result;
    let result = summarize_non_embedded_result(Some("Read"), "permission denied", true);
    assert_eq!(result, "✗ Read failed");
}

#[test]
fn test_summarize_non_embedded_unknown_tool_truncates() {
    use super::summarize_non_embedded_result;
    let long: String = (1..=20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let result = summarize_non_embedded_result(None, &long, false);
    assert!(result.contains("lines omitted"), "未知工具应走截断路径");
}

#[test]
fn test_summarize_non_embedded_empty() {
    use super::summarize_non_embedded_result;
    assert_eq!(summarize_non_embedded_result(Some("Read"), "", false), "");
}
