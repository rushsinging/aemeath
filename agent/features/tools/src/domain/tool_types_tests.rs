use super::*;

#[test]
fn test_tool_result_success_wraps_text_payload() {
    let result: ToolResult = ToolResult::success("ok");

    assert_eq!(result.text, "ok");
    assert!(!result.is_error);
    assert_eq!(result.data, serde_json::Value::Null);
}

// issue #646：AgentProgressKind::Started 构造/字段/PartialEq 验证
#[test]
fn test_agent_progress_started_with_role() {
    let ev = AgentProgressEvent {
        source_context: None,
        sequence: 0,
        kind: AgentProgressKind::Started {
            role: Some("coder".into()),
            model: "Zhipu/glm-5.2".into(),
        },
    };
    match &ev.kind {
        AgentProgressKind::Started { role, model } => {
            assert_eq!(role.as_deref(), Some("coder"));
            assert_eq!(model, "Zhipu/glm-5.2");
        }
        _ => panic!("expected Started"),
    }
}

#[test]
fn test_agent_progress_started_without_role() {
    let ev = AgentProgressEvent {
        source_context: None,
        sequence: 0,
        kind: AgentProgressKind::Started {
            role: None,
            model: "default-model".into(),
        },
    };
    match &ev.kind {
        AgentProgressKind::Started { role, model } => {
            assert!(role.is_none());
            assert_eq!(model, "default-model");
        }
        _ => panic!("expected Started"),
    }
}

#[test]
fn test_agent_progress_kind_partial_eq() {
    let a = AgentProgressKind::Started {
        role: None,
        model: "x".into(),
    };
    let b = AgentProgressKind::Started {
        role: None,
        model: "x".into(),
    };
    assert_eq!(a, b);

    let c = AgentProgressKind::Started {
        role: Some("y".into()),
        model: "x".into(),
    };
    assert_ne!(a, c);

    // 不同变体不相等
    let d = AgentProgressKind::Message { text: "x".into() };
    assert_ne!(a, d);
}

#[test]
fn sub_run_activity_preserves_identity() {
    let identity = SubRunIdentity {
        agent_id: "agent-child-a".to_string(),
        run_id: "run-child-a".to_string(),
        parent_chat_id: "parent-chat".to_string(),
        parent_run_id: "run-main".to_string(),
        spawned_by_tool_call_id: "tool-agent-a".to_string(),
    };
    let event = SubRunActivityEvent {
        identity: identity.clone(),
        sequence: 7,
        kind: SubRunActivityKind::Thinking {
            text: "分析配置".to_string(),
        },
    };

    assert_eq!(event.identity, identity);
    assert_eq!(event.sequence, 7);
    assert!(matches!(
        event.kind,
        SubRunActivityKind::Thinking { ref text } if text == "分析配置"
    ));
}

#[test]
fn test_agent_progress_tool_output_carries_tool_name_and_text() {
    let ev = AgentProgressEvent {
        source_context: None,
        sequence: 1,
        kind: AgentProgressKind::ToolOutput {
            tool_name: "Bash".into(),
            text: "hello".into(),
        },
    };

    match &ev.kind {
        AgentProgressKind::ToolOutput { tool_name, text } => {
            assert_eq!(tool_name, "Bash");
            assert_eq!(text, "hello");
        }
        other => panic!("expected ToolOutput, got {other:?}"),
    }
}

#[test]
fn tool_progress_event_carries_text() {
    let ev = ToolProgressEvent {
        text: "checking PR status…\n".to_string(),
    };
    assert_eq!(ev.text, "checking PR status…\n");
}

#[test]
fn tool_progress_event_partial_eq() {
    let a = ToolProgressEvent {
        text: "line1".into(),
    };
    let b = ToolProgressEvent {
        text: "line1".into(),
    };
    assert_eq!(a, b);

    let c = ToolProgressEvent {
        text: "line2".into(),
    };
    assert_ne!(a, c);
}
