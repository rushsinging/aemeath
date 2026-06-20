//! output.rs 辅助函数单元测试 + 非嵌入/orphan 集成测试。

use super::OutputViewAssembler;
use crate::tui::model::conversation::ids::ToolCallId;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
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
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: ToolCallId::new("tool-orphan"),
        tool_name: "Read".to_string(),
        output: "1\t# 活动中 Feature\n2\t\n3\t|#|标题|\n4\t---\n5\t|8|Memory|".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1, None);
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
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: ToolCallId::new("tool-1"),
        tool_name: "Read".to_string(),
        output: "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });
    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1, None);
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
    let output = (1..=100)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    conversation.apply(ConversationIntent::ObserveToolResult {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: ToolCallId::new("tool-orphan"),
        tool_name: "Bash".to_string(),
        output: output.clone(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1, None);
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
            text_view.text, "✓ Run completed",
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
fn test_summarize_non_embedded_unknown_tool_uses_generic_summary() {
    use super::summarize_non_embedded_result;
    // #87 残留根因：tool_name 未知（id 错位导致 find_tool_name_by_id=None）时，
    // 旧逻辑走 truncate 把完整原始 output 当摘要刷出（带行号正文 + "lines omitted"）。
    // 修复后即使无工具名也只显示通用完成摘要，绝不泄漏正文。
    let long: String = (1..=20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let result = summarize_non_embedded_result(None, &long, false);
    assert!(
        !result.contains("line 10"),
        "未知工具不应刷出原始 output 行，实际: {result}"
    );
    assert!(
        !result.contains("lines omitted"),
        "未知工具不应截断刷出原始内容，实际: {result}"
    );
    assert!(result.starts_with('✓'), "应为通用完成摘要，实际: {result}");
}

#[test]
fn test_summarize_non_embedded_empty() {
    use super::summarize_non_embedded_result;
    assert_eq!(summarize_non_embedded_result(Some("Read"), "", false), "");
}

#[test]
fn test_non_embedded_tool_result_with_unknown_id_does_not_leak_raw_output() {
    // 复现 #87 实测 bug（LEAK-TRACE 日志确认）：ConversationBlock::ToolResult 的 id
    // 在 chats.turns.tool_calls 中找不到（find_tool_name_by_id=None），旧逻辑经
    // truncate 把完整带行号 output 当摘要逐行刷出（正文刷屏）。修复后只显示通用完成摘要。
    use crate::tui::model::conversation::block::ConversationBlock;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};

    let mut conversation = ConversationModel::default();
    // chats 为空 → 没有任何 tool_call 与该 ToolResult id 匹配（模拟 id 错位）。
    let output = (1..=1295)
        .map(|i| format!("{i}\t# 活动中 Bug 行"))
        .collect::<Vec<_>>()
        .join("\n");
    conversation.blocks.push(ConversationBlock::ToolResult {
        id: ToolCallId::new("call_orphaned"),
        chat_id: ChatId::new("chat-orphaned"),
        turn_id: ChatTurnId::new("turn-orphaned"),
        output: output.clone(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1, None);
    let expected_id = ToolCallId::new("call_orphaned");
    let block = vm
        .roots
        .iter()
        .find(|block| block.block_id.contains(&expected_id.to_string()))
        .expect("应有该 ToolResult 对应的块");
    let OutputBlockKind::DiagnosticNotice(text_view) = &block.kind else {
        panic!("应为 DiagnosticNotice");
    };
    assert!(
        !text_view.text.contains("活动中 Bug 行"),
        "不应刷出原始 output 正文，实际: {}",
        text_view.text
    );
    assert!(
        !text_view.text.contains("lines omitted"),
        "不应截断刷出原始内容，实际: {}",
        text_view.text
    );
    assert!(
        text_view.text.starts_with('✓'),
        "应为通用完成摘要，实际: {}",
        text_view.text
    );
}
