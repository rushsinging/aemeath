#[cfg(test)]
use crate::tui::adapter::event_mapping::{sdk_event_to_tui_event, SdkEventMapping};
use crate::tui::adapter::tui_runtime_event::{TuiRuntimeEvent, TuiSubRunActivityKind};

#[test]
fn current_and_legacy_sub_run_started_inputs_normalize_to_canonical_tui_fact() {
    let identity = sdk::SubRunIdentityView {
        agent_id: sdk::AgentId::from_legacy_or_new("agent-sub-a"),
        run_id: sdk::RunId::from_legacy_or_new("run-sub-a"),
        parent_chat_id: sdk::ChatId::from_legacy_or_new("parent-chat"),
        parent_run_id: sdk::RunId::from_legacy_or_new("run-main"),
        spawned_by_tool_call_id: sdk::ToolCallId::from_legacy_or_new("tool-agent-a"),
    };
    let expected_agent_id = identity.agent_id.as_str().to_string();
    let expected_parent_chat_id = identity.parent_chat_id.as_str().to_string();
    let expected_tool_call_id = identity.spawned_by_tool_call_id.as_str().to_string();
    let current = sdk_event_to_tui_event(sdk::ChatEvent::SubRunStarted {
        event: sdk::SubRunStartedEventView {
            identity: identity.clone(),
            sequence: 1,
            role: Some("researcher".to_string()),
            model: "claude-sonnet".to_string(),
        },
    });
    let legacy_expected_agent_id = sdk::ChatId::from_legacy_or_new("agent-sub-a")
        .as_str()
        .to_string();
    let legacy_expected_parent_chat_id = sdk::ChatId::from_legacy_or_new("parent-chat")
        .as_str()
        .to_string();
    let legacy_expected_tool_call_id = sdk::ToolCallId::from_legacy_or_new("tool-agent-a")
        .as_str()
        .to_string();
    let legacy = sdk_event_to_tui_event(sdk::ChatEvent::AgentProgress {
        source_context: sdk::ChatEventContext::new(
            sdk::ChatId::from_legacy_or_new("agent-sub-a"),
            sdk::ChatRunId::from_legacy_or_new("run-sub-a"),
        ),
        attachment_context: sdk::ChatEventContext::new(
            sdk::ChatId::from_legacy_or_new("parent-chat"),
            sdk::ChatRunId::from_legacy_or_new("run-main"),
        ),
        tool_id: sdk::ToolCallId::from_legacy_or_new("tool-agent-a"),
        event: sdk::AgentProgressEventView {
            sequence: 1,
            kind: sdk::AgentProgressKindView::Started {
                role: Some("researcher".to_string()),
                model: "claude-sonnet".to_string(),
            },
        },
    });

    for (mapped, expected_agent_id, expected_parent_chat_id, expected_tool_call_id) in [
        (
            current,
            expected_agent_id,
            expected_parent_chat_id,
            expected_tool_call_id,
        ),
        (
            legacy,
            legacy_expected_agent_id,
            legacy_expected_parent_chat_id,
            legacy_expected_tool_call_id,
        ),
    ] {
        assert!(matches!(
            mapped,
            SdkEventMapping::Runtime(TuiRuntimeEvent::SubRunStarted(event))
                if event.identity.agent_id == expected_agent_id
                    && event.identity.parent_chat_id == expected_parent_chat_id
                    && event.identity.spawned_by_tool_call_id == expected_tool_call_id
                    && event.sequence == 1
                    && event.role.as_deref() == Some("researcher")
                    && event.model == "claude-sonnet"
        ));
    }
}

#[test]
fn sub_run_tool_result_sdk_to_tui_preserves_tool_name() {
    let event = sdk::ChatEvent::SubRunActivity {
        event: sdk::SubRunActivityEventView {
            identity: sdk::SubRunIdentityView {
                agent_id: sdk::AgentId::from_legacy_or_new("agent-child-a"),
                run_id: sdk::RunId::from_legacy_or_new("run-sub-a"),
                parent_chat_id: sdk::ChatId::from_legacy_or_new("parent-chat"),
                parent_run_id: sdk::RunId::from_legacy_or_new("run-main"),
                spawned_by_tool_call_id: sdk::ToolCallId::from_legacy_or_new("tool-agent-a"),
            },
            sequence: 6,
            kind: sdk::SubRunActivityKindView::ToolResult {
                tool_call_id: sdk::ToolCallId::from_legacy_or_new("skill-call"),
                tool_name: "Skill".to_string(),
                output: "SKILL_BODY_SENTINEL".to_string(),
                content: serde_json::json!({"name": "using-superpowers"}),
                is_error: false,
            },
        },
    };

    match sdk_event_to_tui_event(event) {
        SdkEventMapping::Runtime(TuiRuntimeEvent::SubRunActivity(event)) => {
            assert!(matches!(
                event.kind,
                TuiSubRunActivityKind::ToolResult {
                    ref tool_name,
                    ref output,
                    ..
                } if tool_name == "Skill" && output == "SKILL_BODY_SENTINEL"
            ));
        }
        _ => panic!("expected sub run activity"),
    }
}

#[test]
fn sub_run_activity_sdk_to_tui_preserves_identity() {
    let expected_agent_id = sdk::AgentId::from_legacy_or_new("agent-sub-a");
    let expected_run_id = sdk::RunId::from_legacy_or_new("run-sub-a");
    let expected_parent_chat_id = sdk::ChatId::from_legacy_or_new("parent-chat");
    let expected_parent_run_id = sdk::RunId::from_legacy_or_new("run-main");
    let expected_tool_call_id = sdk::ToolCallId::from_legacy_or_new("tool-agent-a");
    let event = sdk::ChatEvent::SubRunActivity {
        event: sdk::SubRunActivityEventView {
            identity: sdk::SubRunIdentityView {
                agent_id: expected_agent_id.clone(),
                run_id: expected_run_id.clone(),
                parent_chat_id: expected_parent_chat_id.clone(),
                parent_run_id: expected_parent_run_id.clone(),
                spawned_by_tool_call_id: expected_tool_call_id.clone(),
            },
            sequence: 5,
            kind: sdk::SubRunActivityKindView::Thinking {
                text: "分析配置".to_string(),
            },
        },
    };

    match sdk_event_to_tui_event(event) {
        SdkEventMapping::Runtime(TuiRuntimeEvent::SubRunActivity(event)) => {
            assert_eq!(event.identity.agent_id, expected_agent_id.as_str());
            assert_eq!(event.identity.run_id.as_str(), expected_run_id.as_str());
            assert_eq!(
                event.identity.parent_chat_id,
                expected_parent_chat_id.as_str()
            );
            assert_eq!(
                event.identity.parent_run_id.as_str(),
                expected_parent_run_id.as_str()
            );
            assert_eq!(
                event.identity.spawned_by_tool_call_id,
                expected_tool_call_id.as_str()
            );
            assert!(matches!(
                event.kind,
                TuiSubRunActivityKind::Thinking { ref text } if text == "分析配置"
            ));
        }
        _ => panic!("expected sub run activity"),
    }
}
