use super::*;
use crate::tui::adapter::agent_event::sanitize::{
    TOOL_RESULT_PREVIEW_LIMIT, TOOL_STREAM_PREVIEW_LIMIT,
};
use crate::tui::app::event::UiTurnContext;
use crate::tui::model::conversation::ids::{ChatId, ChatRunId};
use serde_json::Value;

fn ctx() -> UiTurnContext {
    UiTurnContext {
        chat_id: ChatId::new("chat-test"),
        run_id: ChatRunId::new("turn-test"),
    }
}

fn first_observation(mapping: &AgentEventMapping) -> Option<&ConversationIntent> {
    mapping.conversation.first()
}

fn assert_no_runtime_bind_prelude(mapping: &AgentEventMapping) {
    assert!(
        mapping.conversation.len() <= 1,
        "runtime observations must carry context inline and emit at most one payload intent: {:?}",
        mapping.conversation
    );
}
#[test]
fn workspace_metadata_event_maps_to_matching_workspace_intent() {
    let event =
        UiEvent::WorkspaceMetadataResolved(crate::tui::app::event::WorkspaceMetadataResolved {
            root: "/repo".to_string(),
            revision: 2,
            branch: Some("feature/metadata".to_string()),
            kind: crate::tui::model::conversation::workspace::WorktreeKind::LinkedWorktree,
        });

    let mapping = map_agent_event(&event);

    assert!(matches!(
        mapping.workspace.as_slice(),
        [crate::tui::model::workspace_provider::WorkspaceIntent::ApplyMetadata {
            root,
            revision: 2,
            branch: Some(branch),
            kind: crate::tui::model::conversation::workspace::WorktreeKind::LinkedWorktree,
        }] if root == "/repo" && branch == "feature/metadata"
    ));
}

#[test]
fn test_map_agent_event_runtime_observations_do_not_emit_bind_runtime_turn() {
    let context = ctx();

    let events = vec![
        UiEvent::Text {
            context: context.clone(),
            text: "hello".to_string(),
        },
        UiEvent::Thinking {
            context: context.clone(),
            text: "thinking".to_string(),
        },
        UiEvent::BlockComplete {
            context: context.clone(),
            text: String::new(),
        },
        UiEvent::ToolCallStart {
            context: context.clone(),
            id: sdk::ids::ToolCallId::new("tool-1"),
            provider_id: Some("provider-1".to_string()),
            name: "Read".to_string(),
            index: 0,
        },
        UiEvent::ToolCallUpdate {
            context: context.clone(),
            id: sdk::ids::ToolCallId::new("tool-1"),
            provider_id: Some("provider-1".to_string()),
            name: "Read".to_string(),
            index: 0,
            arguments_delta: Some("{}".to_string()),
            arguments: None,
            status: sdk::ToolCallStatusView::Ready,
        },
        UiEvent::ToolResult {
            context: context.clone(),
            id: sdk::ids::ToolCallId::new("tool-1"),
            provider_id: "provider-1".to_string(),
            tool_name: "Read".to_string(),
            output: "ok".to_string(),
            content: serde_json::json!(null),
            is_error: false,
            images: vec![],
        },
        UiEvent::Done {
            context: context.clone(),
        },
        UiEvent::Cancelled {
            context: context.clone(),
            duration: std::time::Duration::ZERO,
        },
    ];

    for event in &events {
        let mapping = map_agent_event(event);
        assert_no_runtime_bind_prelude(&mapping);
    }
}

#[test]
fn unknown_tool_result_content_is_bounded_before_entering_model() {
    let oversized = "界".repeat(TOOL_RESULT_PREVIEW_LIMIT);
    assert!(oversized.len() > TOOL_RESULT_PREVIEW_LIMIT);
    let mapping = map_agent_event(&UiEvent::ToolResult {
        context: ctx(),
        id: sdk::ids::ToolCallId::new("tool-oversized"),
        provider_id: "provider-oversized".to_string(),
        tool_name: "UnknownTool".to_string(),
        output: oversized.clone(),
        content: serde_json::json!({ "unexpected": oversized }),
        is_error: false,
        images: vec![],
    });

    let Some(ConversationIntent::ToolResult(ToolResult {
        output, content, ..
    })) = first_observation(&mapping)
    else {
        panic!("expected tool result intent");
    };
    assert!(output.len() <= TOOL_RESULT_PREVIEW_LIMIT + 256);
    assert!(output.contains("omitted"));
    let encoded_content = content.to_string();
    assert!(encoded_content.len() <= TOOL_RESULT_PREVIEW_LIMIT + 256);
    assert!(encoded_content.contains("omitted"));
}

#[test]
fn test_map_agent_event_text_to_conversation_intent() {
    let mapping = map_agent_event(&UiEvent::Text {
        context: ctx(),
        text: "hello".to_string(),
    });
    assert!(matches!(
        first_observation(&mapping),
        Some(ConversationIntent::AssistantText(AssistantText { text, .. })) if text == "hello"
    ));
}

#[test]
fn test_map_agent_event_text_sets_generating_phase_with_text_update() {
    let mapping = map_agent_event(&UiEvent::Text {
        context: ctx(),
        text: "hello".to_string(),
    });

    assert!(matches!(
        first_observation(&mapping),
        Some(ConversationIntent::AssistantText(AssistantText { text, .. })) if text == "hello"
    ));
}

#[test]
fn test_map_agent_event_thinking_sets_thinking_phase_with_text_update() {
    let mapping = map_agent_event(&UiEvent::Thinking {
        context: ctx(),
        text: "reason".to_string(),
    });

    assert!(matches!(
        first_observation(&mapping),
        Some(ConversationIntent::ThinkingText(ThinkingText { text, .. })) if text == "reason"
    ));
}

#[test]
fn test_text_maps_directly_to_payload() {
    let mapping = map_agent_event(&UiEvent::Text {
        context: ctx(),
        text: "hello".to_string(),
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::AssistantText(AssistantText { text, .. })] if text == "hello"
    ));
}

#[test]
fn test_thinking_maps_directly_to_payload() {
    let mapping = map_agent_event(&UiEvent::Thinking {
        context: ctx(),
        text: "reason".to_string(),
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::ThinkingText(ThinkingText { text, .. })] if text == "reason"
    ));
}

#[test]
fn test_tool_call_start_maps_directly_to_payload() {
    let mapping = map_agent_event(&UiEvent::ToolCallStart {
        context: ctx(),
        id: sdk::ids::ToolCallId::new("tool-1"),
        provider_id: Some("provider-1".to_string()),
        name: "Write".to_string(),
        index: 0,
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::ToolCallStart(ToolCallStart { name, .. })] if name == "Write"
    ));
}

#[test]
fn test_map_agent_event_usage_to_conversation_intent() {
    let mapping = map_agent_event(&UiEvent::Usage {
        input: 1,
        output: 2,
        last_input: 1,
        elapsed_secs: 1.0,
    });
    assert!(matches!(
        mapping.conversation.first(),
        Some(ConversationIntent::RecordUsage(RecordUsage {
            input_tokens: 1,
            output_tokens: 2,
            last_input_tokens: 1,
            ..
        }))
    ));
    // RecordLiveTps should also be present since elapsed_secs > 0
    assert!(matches!(
        mapping.conversation.get(1),
        Some(ConversationIntent::RecordLiveTps(RecordLiveTps { tps })) if *tps == 2.0
    ));
}

#[test]
fn test_map_agent_event_tool_call_fallback_uses_full_arguments_when_delta_absent() {
    let event = UiEvent::ToolCallUpdate {
        context: ctx(),
        id: sdk::ids::ToolCallId::new("tool-1"),
        provider_id: Some("provider-1".to_string()),
        name: "Read".to_string(),
        index: 0,
        arguments_delta: None,
        arguments: Some(serde_json::json!({ "file_path": "src/lib.rs" })),
        status: sdk::ToolCallStatusView::Ready,
    };
    let mapping = map_agent_event(&event);

    match first_observation(&mapping) {
        Some(ConversationIntent::ToolCallUpdate(ToolCallUpdate { arguments, .. })) => {
            // arguments_delta 为 None 时，fallback 到 arguments JSON 字符串
            assert!(arguments.is_some());
        }
        other => panic!("unexpected mapping: {other:?}"),
    }
}

#[test]
fn test_map_agent_event_error_records_diagnostic_and_hook() {
    let mapping = map_agent_event(&UiEvent::Error("坏了".to_string()));
    assert_eq!(mapping.conversation.len(), 1);
    assert_eq!(mapping.diagnostic.len(), 1);
    assert!(mapping.conversation.len() == 1);
    assert!(mapping.diagnostic.len() == 1);
}

#[test]
fn test_sanitize_edit_arguments_delta_preserves_valid_json() {
    // Edit 参数含超长 old_string/new_string，原始 JSON 远超 512 字节
    let long_old = "x".repeat(400);
    let long_new = "y".repeat(400);
    let raw = format!(
        r#"{{"file_path":"src/main.rs","old_string":"{long_old}","new_string":"{long_new}"}}"#
    );
    assert!(
        raw.len() > TOOL_STREAM_PREVIEW_LIMIT,
        "test precondition: raw JSON should exceed limit"
    );

    let sanitized = sanitize_tool_arguments_delta("Edit", &raw);

    // 核心断言：摘要后仍是合法 JSON
    let parsed: Value =
        serde_json::from_str(&sanitized).expect("sanitized args must be valid JSON");

    // file_path 正确保留
    assert_eq!(
        parsed.get("file_path").and_then(|v| v.as_str()),
        Some("src/main.rs"),
        "file_path must survive sanitization"
    );

    // old_string/new_string 被截断摘要（不再保持原长）
    let old_val = parsed
        .get("old_string")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        old_val.len() < long_old.len(),
        "old_string should be summarized, got {} bytes",
        old_val.len()
    );
    assert!(
        old_val.contains("omitted"),
        "old_string summary should contain 'omitted'"
    );
}

#[test]
fn test_sanitize_partial_json_truncates() {
    let partial = r#"{"file_path":"src/main.rs","old_string":"x"#;
    let sanitized = sanitize_tool_arguments_delta("Edit", partial);
    // 回退模式：不是合法 JSON 但被截断
    assert!(
        sanitized.contains("omitted") || sanitized == partial,
        "partial JSON should be truncated, got: {sanitized}"
    );
}
