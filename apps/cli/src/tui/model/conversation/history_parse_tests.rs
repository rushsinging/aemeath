use super::*;
use crate::tui::adapter::runtime_view::TuiMessageSource;

fn msg(role: &str, content: Vec<TuiContentBlock>) -> TuiChatMessage {
    TuiChatMessage {
        role: role.to_string(),
        content,
        source: TuiMessageSource::User,
        hook_notice: None,
        skill_request: None,
        input_id: None,
    }
}

fn text_block(s: &str) -> TuiContentBlock {
    TuiContentBlock::Text {
        text: s.to_string(),
    }
}

fn thinking_block(s: &str) -> TuiContentBlock {
    TuiContentBlock::Thinking {
        thinking: s.to_string(),
        signature: None,
    }
}

fn tool_use_block(id: &str, name: &str) -> TuiContentBlock {
    TuiContentBlock::ToolUse {
        id: id.to_string(),
        name: name.to_string(),
        input: serde_json::json!({}),
    }
}

fn tool_result_block(id: &str, content: serde_json::Value) -> TuiContentBlock {
    TuiContentBlock::ToolResult {
        tool_use_id: id.to_string(),
        content,
        is_error: false,
        text: None,
    }
}

fn image_block() -> TuiContentBlock {
    TuiContentBlock::Image {
        media_type: "image/png".to_string(),
        base64: "iVBOR".to_string(),
        placeholder: Some("[Image #1]".to_string()),
    }
}

// ── parse: user 分支 ──

#[test]
fn parse_hook_message_projects_typed_notice() {
    let mut message = msg("user", vec![text_block("hook feedback")]);
    let notice = crate::tui::adapter::runtime_view::TuiHookNotice {
        point: "Stop".to_string(),
        kind: crate::tui::adapter::runtime_view::TuiHookNoticeKind::Blocked,
        summary: "blocked".to_string(),
        command: "check.sh".to_string(),
        exit_code: Some(1),
        reason: "exit code 1".to_string(),
        stdout_preview: String::new(),
        stderr_preview: "stderr".to_string(),
        stdout_truncated: false,
        stderr_truncated: false,
        output_file: None,
    };
    let expected_text = notice.display_text();
    message.source = TuiMessageSource::Hook;
    message.hook_notice = Some(notice);

    assert_eq!(
        HistoryDisplayMessage::parse(&message),
        Ok(HistoryDisplayMessage::HookNotice {
            title: "Stop hook blocked".to_string(),
            text: expected_text,
            kind: crate::tui::adapter::runtime_view::TuiHookNoticeKind::Blocked,
        })
    );
}

#[test]
fn parse_system_generated_message_is_not_user_visible_history() {
    let mut message = msg("user", vec![text_block("guidance changed")]);
    message.source = TuiMessageSource::SystemGenerated;

    assert_eq!(
        HistoryDisplayMessage::parse(&message),
        Err(HistoryDisplayParseError::NonUserVisibleMessage)
    );
}

#[test]
fn test_parse_user_text_only() {
    let m = msg("user", vec![text_block("hello")]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Ok(HistoryDisplayMessage::User {
            text: "hello".to_string(),
        })
    );
}

#[test]
fn test_parse_user_multiple_text_concatenated() {
    let m = msg("user", vec![text_block("hello "), text_block("world")]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Ok(HistoryDisplayMessage::User {
            text: "hello world".to_string(),
        })
    );
}

#[test]
fn test_parse_user_tool_result_only_becomes_tool_results() {
    let m = msg(
        "user",
        vec![tool_result_block("t1", serde_json::json!("done"))],
    );
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Ok(HistoryDisplayMessage::ToolResults)
    );
}

#[test]
fn test_parse_user_text_with_tool_result_prefers_user() {
    let m = msg(
        "user",
        vec![
            text_block("question"),
            tool_result_block("t1", serde_json::json!("done")),
        ],
    );
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Ok(HistoryDisplayMessage::User {
            text: "question".to_string(),
        })
    );
}

#[test]
fn test_parse_user_empty_returns_error() {
    let m = msg("user", vec![]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Err(HistoryDisplayParseError::EmptyUserText)
    );
}

#[test]
fn test_parse_user_thinking_unsupported() {
    let m = msg("user", vec![thinking_block("hmm")]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Err(HistoryDisplayParseError::UnsupportedUserBlock(
            "thinking".to_string()
        ))
    );
}

/// #fix-tui-image-input-output：image block 现在按占位符 `[Image #N]` 渲染
/// （保留 round-trip 位置），而非报错。
#[test]
fn test_parse_user_image_renders_placeholder() {
    let m = msg("user", vec![image_block()]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Ok(HistoryDisplayMessage::User {
            text: "[Image #1]".to_string()
        })
    );
}

// ── parse: assistant 分支 ──

#[test]
fn test_parse_assistant_text() {
    let m = msg("assistant", vec![text_block("answer")]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Ok(HistoryDisplayMessage::Assistant {
            blocks: vec![HistoryAssistantBlock::Text("answer".to_string())],
        })
    );
}

#[test]
fn test_parse_assistant_thinking() {
    let m = msg("assistant", vec![thinking_block("plan")]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Ok(HistoryDisplayMessage::Assistant {
            blocks: vec![HistoryAssistantBlock::Thinking("plan".to_string())],
        })
    );
}

#[test]
fn test_parse_assistant_tool_use() {
    let m = msg("assistant", vec![tool_use_block("t1", "Read")]);
    let Ok(HistoryDisplayMessage::Assistant { blocks }) = HistoryDisplayMessage::parse(&m) else {
        panic!("expected Assistant");
    };
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        HistoryAssistantBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "t1");
            assert_eq!(name, "Read");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn test_parse_assistant_mixed_blocks_preserves_order() {
    let m = msg(
        "assistant",
        vec![
            thinking_block("plan"),
            text_block("answer"),
            tool_use_block("t1", "Read"),
        ],
    );
    let Ok(HistoryDisplayMessage::Assistant { blocks }) = HistoryDisplayMessage::parse(&m) else {
        panic!("expected Assistant");
    };
    assert_eq!(blocks.len(), 3);
    assert!(matches!(&blocks[0], HistoryAssistantBlock::Thinking(t) if t == "plan"));
    assert!(matches!(&blocks[1], HistoryAssistantBlock::Text(t) if t == "answer"));
    assert!(matches!(&blocks[2], HistoryAssistantBlock::ToolUse { name, .. } if name == "Read"));
}

#[test]
fn test_parse_assistant_tool_result_unsupported() {
    let m = msg(
        "assistant",
        vec![tool_result_block("t1", serde_json::json!("x"))],
    );
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Err(HistoryDisplayParseError::UnsupportedAssistantBlock(
            "tool_result".to_string()
        ))
    );
}

#[test]
fn test_parse_assistant_image_unsupported() {
    let m = msg("assistant", vec![image_block()]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Err(HistoryDisplayParseError::UnsupportedAssistantBlock(
            "image".to_string()
        ))
    );
}

#[test]
fn test_parse_assistant_empty_returns_error() {
    let m = msg("assistant", vec![]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Err(HistoryDisplayParseError::EmptyAssistantMessage)
    );
}

// ── parse: role 分支 ──

#[test]
fn test_parse_unknown_role_returns_error() {
    let m = msg("system", vec![text_block("notice")]);
    assert_eq!(
        HistoryDisplayMessage::parse(&m),
        Err(HistoryDisplayParseError::UnsupportedRole(
            "system".to_string()
        ))
    );
}

// ── collect_following_tool_results ──

#[test]
fn test_collect_following_tool_results_none_returns_empty() {
    let map = collect_following_tool_results(None);
    assert!(map.is_empty());
}

#[test]
fn test_collect_following_tool_results_extracts_by_id() {
    let next = msg(
        "user",
        vec![
            text_block("ignored"),
            tool_result_block("t1", serde_json::json!("result-1")),
            tool_result_block("t2", serde_json::json!(["err"])),
        ],
    );
    let map = collect_following_tool_results(Some(&next));
    assert_eq!(map.len(), 2);
    assert!(map.contains_key("t1"));
    assert!(map.contains_key("t2"));
}

// ── tool_result_display_text ──

#[test]
fn resumed_tool_result_prefers_persisted_text_for_every_tool() {
    for (tool_name, content) in [
        (
            "Bash",
            serde_json::json!({ "stdout": "raw bash", "exit_code": 0 }),
        ),
        ("Read", serde_json::json!({ "content": "raw read" })),
        ("Grep", serde_json::json!({ "matches": ["raw grep"] })),
        ("Agent", serde_json::json!({ "output": "raw agent" })),
        ("McpTool", serde_json::json!({ "value": "raw mcp" })),
    ] {
        assert_eq!(
            tool_result_display_text(HistoryToolResult {
                content: &content,
                text: Some("human display"),
                is_error: false,
            }),
            "human display",
            "{tool_name} resume 必须统一优先使用持久化 text"
        );
    }
}

#[test]
fn resumed_tool_result_without_text_falls_back_to_legacy_content() {
    let content = serde_json::json!({ "legacy": true });
    assert_eq!(
        tool_result_display_text(HistoryToolResult {
            content: &content,
            text: None,
            is_error: false,
        }),
        content.to_string()
    );
}

// ── tool_result_content_to_string ──

#[test]
fn test_tool_result_content_to_string_string_value() {
    assert_eq!(
        tool_result_content_to_string(&serde_json::json!("hello")),
        "hello"
    );
}

#[test]
fn test_tool_result_content_to_string_array_joins_text_fields() {
    let content = serde_json::json!([
        { "type": "text", "text": "line1" },
        { "type": "text", "text": "line2" }
    ]);
    assert_eq!(tool_result_content_to_string(&content), "line1\nline2");
}

#[test]
fn test_tool_result_content_to_string_object_falls_back_to_to_string() {
    let content = serde_json::json!({ "stdout": "out" });
    assert_eq!(tool_result_content_to_string(&content), content.to_string());
}

// ── normalize_tool_result_content ──

#[test]
fn test_normalize_string_wraps_in_text_object() {
    let result = normalize_tool_result_content(&serde_json::json!("raw"));
    assert_eq!(result, serde_json::json!({ "text": "raw" }));
}

#[test]
fn test_normalize_array_joins_text_fields() {
    let content = serde_json::json!([
        { "type": "text", "text": "a" },
        { "type": "text", "text": "b" }
    ]);
    let result = normalize_tool_result_content(&content);
    assert_eq!(result, serde_json::json!({ "text": "a\nb" }));
}

// ── tool_result_image_count ──

#[test]
fn test_tool_result_image_count_counts_image_type() {
    let content = serde_json::json!([
        { "type": "text", "text": "x" },
        { "type": "image", "source": {} },
        { "type": "image", "source": {} }
    ]);
    assert_eq!(tool_result_image_count(&content), 2);
}

#[test]
fn test_tool_result_image_count_non_array_returns_zero() {
    assert_eq!(tool_result_image_count(&serde_json::json!("text")), 0);
}
