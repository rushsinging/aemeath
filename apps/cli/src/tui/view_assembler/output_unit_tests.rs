//! output.rs 辅助函数单元测试 + 非嵌入/orphan 集成测试。

use super::OutputViewAssembler;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::view_model::{OutputBlockKind, SemanticStyle};

#[test]
fn test_orphan_read_result_shows_summary_not_full_content() {
    // 问题 #87 残留：orphan Read result（结果早于 ToolCall 绑定且未被提升）
    // 不应把完整带行号文件内容刷出（看起来像 LLM 正文），应显示工具摘要，
    // 且颜色为 Success（绿）而非 Warning（橙）。
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "x".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-orphan".to_string(),
        tool_name: "Read".to_string(),
        output: "1\t# 活动中 Feature\n2\t\n3\t|#|标题|\n4\t---\n5\t|8|Memory|".to_string(),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1);
    let orphan = vm
        .roots
        .iter()
        .find(|block| block.block_id.starts_with("orphan-"))
        .expect("应有 orphan block");
    let OutputBlockKind::DiagnosticNotice(text_view) = &orphan.kind else {
        panic!("orphan 应为 DiagnosticNotice");
    };
    assert!(
        !text_view.text.contains("活动中 Feature"),
        "orphan 不应刷出原始文件正文内容，实际: {}",
        text_view.text
    );
    assert!(
        text_view.text.contains("Read"),
        "orphan 应显示工具摘要（含 Read），实际: {}",
        text_view.text
    );
    assert_eq!(
        text_view.style,
        SemanticStyle::Success,
        "非错误 orphan 摘要应为 Success 色而非 Warning"
    );
}

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
fn test_orphan_tool_result_shows_summary_not_raw_output() {
    // OrphanToolResult 路径：tool result 在 tool call 之前到达。
    // 验证完整 output 不被原样透传/截断刷出，而是走工具摘要（#87）。
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
        assert_eq!(
            text_view.text, "✓ Bash completed",
            "orphan 应显示工具摘要而非原始 output，实际: {}",
            text_view.text
        );
        assert!(
            !text_view.text.contains("line 50"),
            "orphan 不应刷出原始 output 行，实际: {}",
            text_view.text
        );
        assert_eq!(
            text_view.style,
            SemanticStyle::Success,
            "非错误 orphan 摘要应为 Success 色"
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
