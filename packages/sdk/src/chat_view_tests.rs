use super::*;

#[test]
fn sub_run_activity_round_trips_without_field_loss() {
    let event = SubRunActivityEventView {
        identity: SubRunIdentityView {
            agent_id: crate::AgentId::from_legacy_or_new("agent-sub-a"),
            run_id: crate::RunId::from_legacy_or_new("run-sub-a"),
            parent_chat_id: crate::ChatId::from_legacy_or_new("parent-chat"),
            parent_run_id: crate::RunId::from_legacy_or_new("run-main"),
            spawned_by_tool_call_id: crate::ToolCallId::from_legacy_or_new("tool-agent-a"),
        },
        sequence: 9,
        kind: SubRunActivityKindView::ToolResult {
            tool_call_id: crate::ToolCallId::from_legacy_or_new("skill-call"),
            tool_name: "Skill".to_string(),
            output: "SKILL_BODY_SENTINEL".to_string(),
            content: serde_json::json!({"name": "using-superpowers"}),
            is_error: false,
        },
    };

    let json = serde_json::to_string(&event).unwrap();
    let restored: SubRunActivityEventView = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, event);
    assert!(matches!(
        restored.kind,
        SubRunActivityKindView::ToolResult { ref tool_name, .. }
            if tool_name == "Skill"
    ));
}

#[test]
fn test_agent_progress_view_supports_message_and_tool_calls() {
    let message = AgentProgressEventView {
        sequence: 1,
        kind: AgentProgressKindView::Message {
            text: "working".to_string(),
        },
    };
    let tools = AgentProgressEventView {
        sequence: 2,
        kind: AgentProgressKindView::ToolCalls {
            calls: vec![AgentToolCallProgressView {
                id: crate::ids::ToolCallId::new_v7(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path":"a.rs"}),
            }],
        },
    };
    let tool_output = AgentProgressEventView {
        sequence: 3,
        kind: AgentProgressKindView::ToolOutput {
            tool_name: "Bash".to_string(),
            text: "stdout".to_string(),
        },
    };

    assert_eq!(message.sequence, 1);
    match message.kind {
        AgentProgressKindView::Message { text } => assert_eq!(text, "working"),
        other => panic!("unexpected kind: {other:?}"),
    }
    match tools.kind {
        AgentProgressKindView::ToolCalls { calls } => {
            assert_eq!(calls[0].name, "Read");
        }
        other => panic!("unexpected kind: {other:?}"),
    }
    match tool_output.kind {
        AgentProgressKindView::ToolOutput { tool_name, text } => {
            assert_eq!(tool_name, "Bash");
            assert_eq!(text, "stdout");
        }
        other => panic!("unexpected kind: {other:?}"),
    }
}
#[test]
fn test_agent_progress_display_tool_calls() {
    let event = AgentProgressEventView {
        sequence: 1,
        kind: AgentProgressKindView::ToolCalls {
            calls: vec![
                AgentToolCallProgressView {
                    id: crate::ids::ToolCallId::new_v7(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({"command": "ls"}),
                },
                AgentToolCallProgressView {
                    id: crate::ids::ToolCallId::new_v7(),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path": "TODO.md"}),
                },
            ],
        },
    };
    assert_eq!(format!("{event}"), "Bash, Read");
}

#[test]
fn test_agent_progress_display_message() {
    let event = AgentProgressEventView {
        sequence: 2,
        kind: AgentProgressKindView::Message {
            text: "分析完成".to_string(),
        },
    };
    assert_eq!(format!("{event}"), "分析完成");
}

#[test]
fn test_tool_progress_event_view_carries_text() {
    let view = ToolProgressEventView {
        text: "checking…\n".to_string(),
    };
    assert_eq!(view.text, "checking…\n");
}

#[test]
fn test_workspace_context_view_keeps_paths() {
    let view = WorkspaceContextView {
        path_base: "/repo/sub".into(),
        workspace_root: "/repo".into(),
        context_stack: vec![WorkspaceStackEntryView {
            path_base: "/repo".into(),
            workspace_root: "/repo".into(),
        }],
    };

    assert_eq!(view.path_base.to_string_lossy(), "/repo/sub");
    assert_eq!(view.workspace_root.to_string_lossy(), "/repo");
    assert_eq!(view.context_stack.len(), 1);
}

#[test]
fn test_option_item_title_only() {
    let item = OptionItem::title_only("Yes".to_string());
    assert_eq!(item.title, "Yes");
    assert!(item.description.is_none());
}

#[test]
fn test_option_item_with_description() {
    let item = OptionItem::new("Deploy", "Push to production");
    assert_eq!(item.title, "Deploy");
    assert_eq!(item.description.as_deref(), Some("Push to production"));
}

#[test]
fn test_option_item_serialize_deserialize_string_compat() {
    // 向后兼容：纯字符串应反序列化为 title_only
    let json = serde_json::json!("Simple option");
    let item: OptionItem = serde_json::from_value(json).unwrap();
    assert_eq!(item.title, "Simple option");
    assert!(item.description.is_none());
}

#[test]
fn test_option_item_serialize_deserialize_object() {
    let json = serde_json::json!({"title": "Go", "description": "Proceed"});
    let item: OptionItem = serde_json::from_value(json).unwrap();
    assert_eq!(item.title, "Go");
    assert_eq!(item.description, Some("Proceed".to_string()));
}

#[test]
fn test_option_item_serialize_outputs_object() {
    let item = OptionItem::new("Test", "Desc");
    let val = serde_json::to_value(&item).unwrap();
    assert_eq!(val["title"], "Test");
    assert_eq!(val["description"], "Desc");
}
