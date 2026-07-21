use super::*;
use crate::tui::adapter::agent_event::sanitize::TOOL_STREAM_PREVIEW_LIMIT;
use crate::tui::app::event::UiTurnContext;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
use serde_json::Value;

fn ctx() -> UiTurnContext {
    UiTurnContext {
        chat_id: ChatId::new("chat-test"),
        turn_id: ChatTurnId::new("turn-test"),
    }
}

fn first_observation(mapping: &AgentEventMapping) -> Option<&ConversationIntent> {
    mapping
        .conversation
        .iter()
        .find(|intent| !matches!(intent, ConversationIntent::ClearModelStreamPlaceholder(_)))
}

fn assert_no_runtime_bind_prelude(mapping: &AgentEventMapping) {
    assert!(
        matches!(
            mapping.conversation.as_slice(),
            [ConversationIntent::ClearModelStreamPlaceholder(_), _] | [_]
        ),
        "runtime observations must carry context inline and emit at most one placeholder clear plus one payload intent: {:?}",
        mapping.conversation
    );
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
        },
    ];

    for event in &events {
        let mapping = map_agent_event(event);
        assert_no_runtime_bind_prelude(&mapping);
    }
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
fn test_model_stream_waiting_maps_to_placeholder() {
    let mapping = map_agent_event(&UiEvent::ModelStreamWaiting {
        context: ctx(),
        elapsed_secs: 10,
        phase: "thinking".to_string(),
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::UpsertModelStreamPlaceholder(UpsertModelStreamPlaceholder {
            placeholder
        })] if placeholder.elapsed_secs == 10 && placeholder.phase == "thinking"
    ));
}

#[test]
fn test_text_clears_model_stream_placeholder_before_payload() {
    let mapping = map_agent_event(&UiEvent::Text {
        context: ctx(),
        text: "hello".to_string(),
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::ClearModelStreamPlaceholder(_), ConversationIntent::AssistantText(AssistantText { text, .. })]
            if text == "hello"
    ));
}

#[test]
fn test_thinking_clears_model_stream_placeholder_before_payload() {
    let mapping = map_agent_event(&UiEvent::Thinking {
        context: ctx(),
        text: "reason".to_string(),
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::ClearModelStreamPlaceholder(_), ConversationIntent::ThinkingText(ThinkingText { text, .. })]
            if text == "reason"
    ));
}

#[test]
fn test_tool_call_start_clears_model_stream_placeholder_before_payload() {
    let mapping = map_agent_event(&UiEvent::ToolCallStart {
        context: ctx(),
        id: sdk::ids::ToolCallId::new("tool-1"),
        provider_id: Some("provider-1".to_string()),
        name: "Write".to_string(),
        index: 0,
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::ClearModelStreamPlaceholder(_), ConversationIntent::ToolCallStart(ToolCallStart { name, .. })]
            if name == "Write"
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
    assert!(matches!(
        mapping.effects.first(),
        Some(Effect::RunHook { .. })
    ));
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

mod started_tests {
    use super::*;
    use crate::tui::app::event::UiTurnContext;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
    use sdk::AgentProgressEventView;
    use sdk::AgentProgressKindView;

    fn ctx() -> UiTurnContext {
        UiTurnContext {
            chat_id: ChatId::new("chat-test"),
            turn_id: ChatTurnId::new("turn-test"),
        }
    }

    fn started_event(role: Option<&str>, model: &str) -> UiEvent {
        UiEvent::AgentProgress {
            context: ctx(),
            tool_id: ToolCallId::new("tool-1"),
            event: AgentProgressEventView {
                sequence: 0,
                kind: AgentProgressKindView::Started {
                    role: role.map(|s| s.to_string()),
                    model: model.to_string(),
                },
            },
        }
    }

    #[test]
    fn test_started_event_maps_to_update_agent_meta() {
        let tool_id = ToolCallId::new("tool-1");
        let ev = UiEvent::AgentProgress {
            context: ctx(),
            tool_id: tool_id.clone(),
            event: AgentProgressEventView {
                sequence: 0,
                kind: AgentProgressKindView::Started {
                    role: Some("coder".to_string()),
                    model: "Zhipu/glm-5.2".to_string(),
                },
            },
        };
        let mapping = map_agent_event(&ev);
        assert_eq!(mapping.conversation.len(), 1);
        match &mapping.conversation[0] {
            ConversationIntent::UpdateAgentMeta(UpdateAgentMeta {
                role,
                model,
                tool_id: got_id,
                ..
            }) => {
                assert_eq!(role.as_deref(), Some("coder"));
                assert_eq!(model, "Zhipu/glm-5.2");
                assert_eq!(got_id, &tool_id);
            }
            other => panic!("expected UpdateAgentMeta, got {other:?}"),
        }
    }

    #[test]
    fn test_started_event_without_role_maps_to_update_agent_meta() {
        let ev = started_event(None, "fallback-model");
        let mapping = map_agent_event(&ev);
        match &mapping.conversation[0] {
            ConversationIntent::UpdateAgentMeta(UpdateAgentMeta { role, model, .. }) => {
                assert!(role.is_none());
                assert_eq!(model, "fallback-model");
            }
            other => panic!("expected UpdateAgentMeta, got {other:?}"),
        }
    }

    #[test]
    fn test_agent_progress_tool_calls_use_tool_display_headers() {
        let ev = UiEvent::AgentProgress {
            context: ctx(),
            tool_id: ToolCallId::new("agent-tool"),
            event: AgentProgressEventView {
                sequence: 1,
                kind: AgentProgressKindView::ToolCalls {
                    calls: vec![sdk::AgentToolCallProgressView {
                        id: sdk::ids::ToolCallId::new("read-1"),
                        name: "Read".to_string(),
                        input: serde_json::json!({
                            "file_path": "/repo/src/main.rs",
                            "offset": 9,
                            "limit": 3
                        }),
                    }],
                },
            },
        };

        let mapping = map_agent_event_with_tool_header(&ev, |name, input| {
            crate::tui::render::output::tool_display::format_subagent_tool_header(name, input, None)
        });
        match &mapping.conversation[0] {
            ConversationIntent::RecordAgentProgress(RecordAgentProgress { message, .. }) => {
                assert_eq!(message, "→ Read /repo/src/main.rs 10:12\n");
                assert!(!message.contains("file_path"));
                assert!(!message.contains('{'));
            }
            other => panic!("expected RecordAgentProgress, got {other:?}"),
        }
    }

    #[test]
    fn test_agent_progress_tool_calls_keep_each_tool_on_separate_line() {
        let ev = UiEvent::AgentProgress {
            context: ctx(),
            tool_id: ToolCallId::new("agent-tool"),
            event: AgentProgressEventView {
                sequence: 1,
                kind: AgentProgressKindView::ToolCalls {
                    calls: vec![
                        sdk::AgentToolCallProgressView {
                            id: sdk::ids::ToolCallId::new("glob-1"),
                            name: "Glob".to_string(),
                            input: serde_json::json!({"pattern":"apps/**/*.rs"}),
                        },
                        sdk::AgentToolCallProgressView {
                            id: sdk::ids::ToolCallId::new("grep-1"),
                            name: "Grep".to_string(),
                            input: serde_json::json!({"pattern":"activity_lines","path":"apps/cli/src"}),
                        },
                    ],
                },
            },
        };

        let mapping = map_agent_event_with_tool_header(&ev, |name, input| {
            crate::tui::render::output::tool_display::format_subagent_tool_header(name, input, None)
        });
        match &mapping.conversation[0] {
            ConversationIntent::RecordAgentProgress(RecordAgentProgress { message, .. }) => {
                assert_eq!(
                    message,
                    "→ Find apps/**/*.rs\n→ Search /activity_lines/ in apps/cli/src\n"
                );
            }
            other => panic!("expected RecordAgentProgress, got {other:?}"),
        }
    }

    #[test]
    fn test_agent_progress_unknown_tool_fallback_truncates_json_and_ends_with_newline() {
        let ev = UiEvent::AgentProgress {
            context: ctx(),
            tool_id: ToolCallId::new("agent-tool"),
            event: AgentProgressEventView {
                sequence: 1,
                kind: AgentProgressKindView::ToolCalls {
                    calls: vec![sdk::AgentToolCallProgressView {
                        id: sdk::ids::ToolCallId::new("unknown-1"),
                        name: "UnknownTool".to_string(),
                        input: serde_json::json!({"very_long_key":"x".repeat(200)}),
                    }],
                },
            },
        };

        let mapping = map_agent_event(&ev);
        match &mapping.conversation[0] {
            ConversationIntent::RecordAgentProgress(RecordAgentProgress { message, .. }) => {
                assert!(message.starts_with("→ UnknownTool "));
                assert!(message.ends_with("\n"));
                assert!(message.contains("..."));
                assert!(message.len() < 140);
            }
            other => panic!("expected RecordAgentProgress, got {other:?}"),
        }
    }

    #[test]
    fn test_non_started_event_maps_to_record_agent_progress() {
        let ev = UiEvent::AgentProgress {
            context: ctx(),
            tool_id: ToolCallId::new("tool-1"),
            event: AgentProgressEventView {
                sequence: 1,
                kind: AgentProgressKindView::Message {
                    text: "working".to_string(),
                },
            },
        };
        let mapping = map_agent_event(&ev);
        match &mapping.conversation[0] {
            ConversationIntent::RecordAgentProgress(RecordAgentProgress { message, .. }) => {
                assert_eq!(message, "working\n");
            }
            other => panic!("expected RecordAgentProgress, got {other:?}"),
        }
    }

    #[test]
    fn test_agent_progress_tool_output_is_not_rendered_as_activity() {
        let ev = UiEvent::AgentProgress {
            context: ctx(),
            tool_id: ToolCallId::new("tool-1"),
            event: AgentProgressEventView {
                sequence: 2,
                kind: AgentProgressKindView::ToolOutput {
                    tool_name: "Bash".to_string(),
                    text: "stdout should stay hidden".to_string(),
                },
            },
        };
        let mapping = map_agent_event(&ev);
        assert!(
            mapping.conversation.is_empty(),
            "ToolOutput 不应进入 conversation activity，实际: {:?}",
            mapping.conversation
        );
    }
}

#[test]
fn test_hook_message_maps_to_info_notice_with_typed_metadata() {
    let mapping = map_agent_event(&UiEvent::HookMessage(sdk::HookMessageView {
        point: "PreToolUse".to_string(),
        source: "matcher:Bash".to_string(),
        execution_ordinal: 2,
        attempt: 3,
        kind: sdk::HookMessageKindView::AdditionalContext,
        text: "Use the checked-in formatter.".to_string(),
    }));

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::AppendHookNotice(AppendHookNotice { content })]
            if content.kind == crate::tui::model::conversation::block::HookNoticeKind::Info
                && content.title == "Hook context: PreToolUse"
                && content.body == "Use the checked-in formatter."
                && content.details.as_deref() == Some("Source: matcher:Bash\nExecution: 2\nAttempt: 3")
    ));
}

#[test]
fn test_hook_message_with_empty_text_is_not_rendered() {
    let mapping = map_agent_event(&UiEvent::HookMessage(sdk::HookMessageView {
        point: "PreToolUse".to_string(),
        source: "matcher:Bash".to_string(),
        execution_ordinal: 0,
        attempt: 1,
        kind: sdk::HookMessageKindView::SystemMessage,
        text: " <system-reminder>\n</system-reminder> ".to_string(),
    }));

    assert!(mapping.conversation.is_empty());
}

// ── #1106：runtime 允许发空 SystemMessage，TUI（ACL 层）不渲染 ──

#[test]
fn test_empty_system_message_from_runtime_is_dropped() {
    // runtime 侧 `if let Some(ctx) = json.additional_context` 只判 Option 不判空串，
    // 允许发空消息；ACL 层负责丢弃，避免空 block 渲染成空行。
    for payload in [
        "",
        "   ",
        "\n\n",
        "<system-reminder></system-reminder>",
        "<system-reminder>\n  \n</system-reminder>",
    ] {
        let mapping = map_agent_event(&UiEvent::SystemMessage(payload.to_string()));
        assert!(
            mapping.conversation.is_empty(),
            "空 SystemMessage 不应产生 intent: {payload:?}"
        );
    }
}

#[test]
fn test_non_empty_system_message_is_mapped() {
    let mapping = map_agent_event(&UiEvent::SystemMessage("hello".to_string()));
    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::AppendSystemMessage(AppendSystemMessage { text })] if text == "hello"
    ));
}

#[test]
fn test_system_reminder_envelope_with_content_is_kept() {
    // 信封剥离后仍有内容 → 必须保留（剥离由 model 层做，ACL 只判空）。
    let mapping = map_agent_event(&UiEvent::SystemMessage(
        "<system-reminder>real content</system-reminder>".to_string(),
    ));
    assert_eq!(mapping.conversation.len(), 1, "非空信封应保留");
}
