#[cfg(test)]
use crate::tui::adapter::event_mapping::{sdk_event_to_tui_event, SdkEventMapping};
use crate::tui::adapter::tui_runtime_event::{TuiChildRunActivityKind, TuiRuntimeEvent};

#[test]
fn child_run_tool_result_sdk_to_tui_preserves_tool_name() {
    let event = sdk::ChatEvent::ChildRunActivity {
        event: sdk::ChildRunActivityEventView {
            identity: sdk::ChildRunIdentityView {
                agent_id: sdk::AgentId::from_legacy_or_new("agent-child-a"),
                run_id: sdk::RunId::from_legacy_or_new("run-child-a"),
                parent_run_id: sdk::RunId::from_legacy_or_new("run-main"),
                spawned_by_tool_call_id: sdk::ToolCallId::from_legacy_or_new("tool-agent-a"),
            },
            sequence: 6,
            kind: sdk::ChildRunActivityKindView::ToolResult {
                tool_call_id: sdk::ToolCallId::from_legacy_or_new("skill-call"),
                tool_name: "Skill".to_string(),
                output: "SKILL_BODY_SENTINEL".to_string(),
                content: serde_json::json!({"name": "using-superpowers"}),
                is_error: false,
            },
        },
    };

    match sdk_event_to_tui_event(event) {
        SdkEventMapping::Runtime(TuiRuntimeEvent::ChildRunActivity(event)) => {
            assert!(matches!(
                event.kind,
                TuiChildRunActivityKind::ToolResult {
                    ref tool_name,
                    ref output,
                    ..
                } if tool_name == "Skill" && output == "SKILL_BODY_SENTINEL"
            ));
        }
        _ => panic!("expected child run activity"),
    }
}

#[test]
fn child_run_activity_sdk_to_tui_preserves_identity() {
    let event = sdk::ChatEvent::ChildRunActivity {
        event: sdk::ChildRunActivityEventView {
            identity: sdk::ChildRunIdentityView {
                agent_id: sdk::AgentId::from_legacy_or_new("agent-child-a"),
                run_id: sdk::RunId::from_legacy_or_new("run-child-a"),
                parent_run_id: sdk::RunId::from_legacy_or_new("run-main"),
                spawned_by_tool_call_id: sdk::ToolCallId::from_legacy_or_new("tool-agent-a"),
            },
            sequence: 5,
            kind: sdk::ChildRunActivityKindView::Thinking {
                text: "分析配置".to_string(),
            },
        },
    };

    match sdk_event_to_tui_event(event) {
        SdkEventMapping::Runtime(TuiRuntimeEvent::ChildRunActivity(event)) => {
            assert_eq!(
                event.identity.agent_id,
                sdk::AgentId::from_legacy_or_new("agent-child-a").as_str()
            );
            assert_eq!(
                event.identity.run_id.as_str(),
                sdk::RunId::from_legacy_or_new("run-child-a").as_str()
            );
            assert_eq!(
                event.identity.parent_run_id.as_str(),
                sdk::RunId::from_legacy_or_new("run-main").as_str()
            );
            assert_eq!(
                event.identity.spawned_by_tool_call_id,
                sdk::ToolCallId::from_legacy_or_new("tool-agent-a").as_str()
            );
            assert!(matches!(
                event.kind,
                TuiChildRunActivityKind::Thinking { ref text } if text == "分析配置"
            ));
        }
        _ => panic!("expected child run activity"),
    }
}
