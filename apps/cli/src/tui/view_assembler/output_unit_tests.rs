//! output.rs 辅助函数单元测试 + 非嵌入/orphan 集成测试。

use super::OutputViewAssembler;
use crate::tui::model::conversation::ids::ToolCallId;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::view_model::{OutputBlockKind, SemanticStyle};

#[test]
fn test_orphan_read_result_shows_summary_not_full_content() {
    // 问题 #87 残留：orphan Read result（结果早于 ToolCall 绑定且未被提升）
    // 不应把完整带行号文件内容刷出（看起来像 LLM 正文），应显示工具摘要，
    // 且颜色为 Success（绿）而非 Warning（橙）。
    let mut conversation = ConversationModel::default();
    conversation.apply(StartChat {
        submission: "x".to_string(),
    });
    conversation.apply(ToolResult {
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
    conversation.apply(StartChat {
        submission: "search".to_string(),
    });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
        model_id: None,
        role: None,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    conversation.apply(ToolResult {
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
    conversation.apply(StartChat {
        submission: "search".to_string(),
    });
    let output = (1..=100)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    conversation.apply(ToolResult {
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

/// A4.5: blocks fallback 已删除。timeline ToolResult 指向不存在的 tool_call_id 时，
/// assembler 静默跳过（find_tool_call → None → continue），不产出任何块。
#[test]
fn test_timeline_tool_result_with_unknown_id_is_silently_skipped() {
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};

    let mut conversation = ConversationModel::default();
    // 直接向 timeline 注入一个 ToolResult，但 chats 中没有对应的 tool_call。
    // A4.5 后：find_tool_call → None → continue，不产出任何块（不泄漏原始内容）。
    conversation.timeline.push_tool_result_ref(
        ChatId::new("chat-x"),
        ChatTurnId::new("turn-x"),
        ToolCallId::new("call-missing"),
    );

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 1, None);
    assert!(
        vm.roots.is_empty(),
        "未知 tool_call_id 的 ToolResult 不应产出任何块，实际: {:?}",
        vm.roots.iter().map(|b| &b.block_id).collect::<Vec<_>>()
    );
}

#[test]
fn test_tool_index_call_matches_linear_scan() {
    use super::ToolIndex;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};

    use crate::tui::model::conversation::model::ConversationModel;

    let mut conv = ConversationModel::default();
    let chat = ChatId::new("c1");
    let turn = ChatTurnId::new("t1");
    let tool = ToolCallId::new("tool-1");
    conv.apply(ToolCallStart {
        chat_id: chat.clone(),
        turn_id: turn.clone(),
        id: tool.clone(),
        provider_id: Some("p".to_string()),
        name: "Read".to_string(),
        index: 0,
        model_id: None,
        role: None,
    });

    let index = ToolIndex::build(&conv);
    let via_index = index.call(&chat, &turn, &tool).map(|c| c.name.clone());
    assert_eq!(via_index.as_deref(), Some("Read"), "索引应命中已登记 tool");
    assert!(
        index
            .call(&chat, &turn, &ToolCallId::new("missing"))
            .is_none(),
        "未登记 tool 应返回 None"
    );
}

/// A4.5: result_block 已删除；tool result 数据现在通过 ToolIndex.call → call.result 读取。
/// 此测试验证 ToolIndex.call 能正确索引到已完成工具的 result payload（四字段全部匹配）。
#[test]
fn test_tool_index_call_result_payload_matches_observed_values() {
    use super::ToolIndex;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};

    use crate::tui::model::conversation::model::ConversationModel;
    use crate::tui::model::conversation::tool_call::ToolCallStatus;

    let mut conv = ConversationModel::default();
    let chat = ChatId::new("c1");
    let turn = ChatTurnId::new("t1");
    let tool = ToolCallId::new("tool-r1");

    conv.apply(ToolCallStart {
        chat_id: chat.clone(),
        turn_id: turn.clone(),
        id: tool.clone(),
        provider_id: Some("prov-1".to_string()),
        name: "Bash".to_string(),
        index: 0,
        model_id: None,
        role: None,
    });
    conv.apply(ToolCallUpdate {
        chat_id: chat.clone(),
        turn_id: turn.clone(),
        id: tool.clone(),
        provider_id: Some("prov-1".to_string()),
        name: "Bash".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let expected_output = "cmd output line";
    let expected_content = serde_json::json!({ "text": "cmd output line" });
    let expected_is_error = false;
    let expected_image_count: usize = 2;
    conv.apply(ToolResult {
        chat_id: chat.clone(),
        turn_id: turn.clone(),
        provider_id: "prov-1".to_string(),
        id: tool.clone(),
        tool_name: "Bash".to_string(),
        output: expected_output.to_string(),
        content: expected_content.clone(),
        is_error: expected_is_error,
        image_count: expected_image_count,
    });

    let index = ToolIndex::build(&conv);

    // A4.5 路径：通过 call → call.result 读取 payload（不再走 result_block）
    let call = index
        .call(&chat, &turn, &tool)
        .expect("已登记 tool call 应命中索引");
    let payload = call
        .result
        .as_ref()
        .expect("已完成 tool call 应有 result payload");
    assert_eq!(payload.output, expected_output, "output 应匹配");
    assert_eq!(payload.content, expected_content, "content 应匹配");
    assert_eq!(payload.is_error, expected_is_error, "is_error 应匹配");
    assert_eq!(
        payload.image_count, expected_image_count,
        "image_count 应匹配"
    );

    // 边界路径：未登记的 tool_id 应返回 None
    assert!(
        index
            .call(&chat, &turn, &ToolCallId::new("no-such-tool"))
            .is_none(),
        "未登记 tool 应返回 None"
    );
}

/// A4.5 集成断言：独立（非内嵌）ToolResult 渲染路径
///
/// 触发条件：output 为空 → `tool_result_is_embedded` = false → 走 standalone DiagnosticNotice 分支。
/// 断言：is_error=true → style=Error；image_count>0 → 文本含 `[图片: N]`。
#[test]
fn test_non_embedded_tool_result_error_with_image_count_renders_correctly() {
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};

    let mut conv = ConversationModel::default();
    let chat = ChatId::new("chat-1");
    let turn = ChatTurnId::new("turn-1");
    let tool = ToolCallId::new("tool-img");

    // 注册 tool call，使 find_tool_call 能命中（tool_result_is_embedded 需要）。
    conv.apply(ToolCallStart {
        chat_id: chat.clone(),
        turn_id: turn.clone(),
        id: tool.clone(),
        provider_id: Some("prov-1".to_string()),
        name: "Bash".to_string(),
        index: 0,
        model_id: None,
        role: None,
    });
    conv.apply(ToolCallUpdate {
        chat_id: chat.clone(),
        turn_id: turn.clone(),
        id: tool.clone(),
        provider_id: Some("prov-1".to_string()),
        name: "Bash".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    // output 为空 → tool_result_is_embedded=false → 触发非嵌入渲染分支。
    // is_error=true、image_count=2 → style=Error、文本含 "[图片: 2]"。
    conv.apply(ToolResult {
        chat_id: chat.clone(),
        turn_id: turn.clone(),
        provider_id: "prov-1".to_string(),
        id: tool.clone(),
        tool_name: "Bash".to_string(),
        output: String::new(),
        content: serde_json::json!({}),
        is_error: true,
        image_count: 2,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conv, 1, None);

    // 应产出一个 root-level DiagnosticNotice（非嵌入路径）。
    let notice = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::DiagnosticNotice(tv) => Some(tv),
            _ => None,
        })
        .expect("非嵌入 ToolResult 应产出 DiagnosticNotice");

    assert_eq!(
        notice.style,
        SemanticStyle::Error,
        "is_error=true 时 style 应为 Error，实际: {:?}",
        notice.style
    );
    assert!(
        notice.text.contains("[图片: 2]"),
        "image_count=2 时文本应含 \"[图片: 2]\"，实际: {:?}",
        notice.text
    );
}
