use crate::tui::adapter::tui_runtime_event::{
    TuiRunContext, TuiRunStepEvent, TuiRuntimeEvent, TuiSubRunActivity, TuiSubRunActivityKind,
    TuiSubRunIdentity, TuiSubRunStarted, TuiToolCallStatus,
};

use crate::tui::model::output_timeline::OutputTimelineItem;

use super::super::testing::TuiScenarioHarness;

#[test]
fn adopted_skill_request_renders_raw_input_as_normal_user_message() {
    let mut harness = TuiScenarioHarness::new(120, 40);
    harness.runtime_event(TuiRuntimeEvent::UserMessagesAdopted {
        items: vec![crate::tui::adapter::runtime_view::TuiChatMessage {
            role: "user".to_string(),
            content: vec![crate::tui::adapter::runtime_view::TuiContentBlock::text(
                "<skill-request>LLM_ONLY</skill-request>",
            )],
            input_id: Some("skill-input".to_string()),
            source: crate::tui::adapter::runtime_view::TuiMessageSource::SkillRequest,
            hook_notice: None,
            skill_request: Some(crate::tui::adapter::runtime_view::TuiSkillRequestMetadata {
                skill: "superpowers:brainstorming".to_string(),
                arguments: "feature scope".to_string(),
                raw_input: "/superpowers:brainstorming feature scope".to_string(),
            }),
        }],
        queued: Vec::new(),
    });
    harness.render();

    let screen = harness.screen();
    assert!(
        screen.contains("/superpowers:brainstorming feature scope"),
        "Skill 用户消息必须显示 raw_input：\n{screen}"
    );
    assert!(
        screen.contains("> /superpowers:brainstorming feature scope"),
        "Skill 用户消息必须使用正常 UserMessage gutter：\n{screen}"
    );
    for hidden in [
        "<skill-request>",
        "LLM_ONLY",
        "Skill superpowers:brainstorming",
    ] {
        assert!(
            !screen.contains(hidden),
            "Skill 用户消息泄漏或错误显示 {hidden}：\n{screen}"
        );
    }
}

#[test]
fn child_progress_attaches_to_parent_agent_block_without_leaking_into_main_timeline() {
    let parent_context = TuiRunContext {
        chat_id: "parent-chat".to_string(),
        run_id: "parent-turn".to_string(),
    };
    let child_context = TuiRunContext {
        chat_id: "child-chat".to_string(),
        run_id: "child-turn".to_string(),
    };
    let tool_id = "agent-tool".to_string();
    let marker = "child-private-progress";
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
        context: parent_context.clone(),
        id: tool_id.clone(),
        provider_id: Some("provider-agent-tool".to_string()),
        name: "Agent".to_string(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallStateChanged {
        context: parent_context.clone(),
        id: tool_id.clone(),
        provider_id: Some("provider-agent-tool".to_string()),
        name: "Agent".to_string(),
        index: 0,
        arguments: Some(serde_json::json!({"role":"coder","prompt":"write hello.rs"})),
        status: TuiToolCallStatus::Ready,
    });
    let sub_run_identity = TuiSubRunIdentity {
        agent_id: child_context.chat_id.clone(),
        run_id: crate::tui::model::conversation::interaction::UiRunId::from(
            child_context.run_id.as_str(),
        ),
        parent_chat_id: parent_context.chat_id.clone(),
        parent_run_id: crate::tui::model::conversation::interaction::UiRunId::from(
            parent_context.run_id.as_str(),
        ),
        spawned_by_tool_call_id: tool_id.clone(),
    };
    harness.runtime_event(TuiRuntimeEvent::SubRunStarted(TuiSubRunStarted {
        identity: sub_run_identity.clone(),
        sequence: 0,
        role: Some("coder".to_string()),
        model: "MiniMax/MiniMax-M3".to_string(),
    }));
    harness.runtime_event(TuiRuntimeEvent::SubRunActivity(TuiSubRunActivity {
        identity: sub_run_identity,
        sequence: 1,
        sequence_index: 0,
        kind: TuiSubRunActivityKind::Text {
            text: marker.to_string(),
        },
    }));
    harness.render();

    let agent_call = harness
        .app
        .model
        .conversation
        .chats
        .iter()
        .flat_map(|chat| &chat.runs)
        .flat_map(|turn| &turn.tool_calls)
        .find(|call| call.name == "Agent")
        .expect("parent Agent tool call should exist");
    assert_eq!(agent_call.activities, vec![marker.to_string()]);
    assert!(
        harness
            .app
            .model
            .conversation
            .timeline
            .items()
            .iter()
            .all(|item| !matches!(item, OutputTimelineItem::AgentProgress { .. })),
        "child progress must remain inline in the parent Agent block"
    );
    assert_eq!(harness.screen().matches(marker).count(), 1);
    harness.assert_idle();
}

#[test]
fn sub_run_skill_result_is_hidden_while_visible_tool_result_renders() {
    let parent_context = TuiRunContext {
        chat_id: "parent-chat".to_string(),
        run_id: "parent-run".to_string(),
    };
    let identity = TuiSubRunIdentity {
        agent_id: "researcher".to_string(),
        run_id: "child-run".into(),
        parent_chat_id: "parent-chat".to_string(),
        parent_run_id: "parent-run".into(),
        spawned_by_tool_call_id: "agent-tool".to_string(),
    };
    let mut harness = TuiScenarioHarness::new(120, 40);
    harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
        context: parent_context,
        id: "agent-tool".to_string(),
        provider_id: Some("provider-agent-tool".to_string()),
        name: "Agent".to_string(),
        index: 0,
    });
    for (sequence, kind) in [
        TuiSubRunActivityKind::ToolCall {
            id: "skill-call".to_string(),
            name: "Skill".to_string(),
            input: serde_json::json!({"skill": "superpowers:using-superpowers"}),
        },
        TuiSubRunActivityKind::ToolResult {
            tool_call_id: "skill-call".to_string(),
            tool_name: "Skill".to_string(),
            output: "SKILL_BODY_SENTINEL\n<skill-request>LLM_ONLY</skill-request>\n<system-reminder>LLM_ONLY</system-reminder>".to_string(),
            content: serde_json::json!({"name": "superpowers:using-superpowers"}),
            is_error: false,
        },
        TuiSubRunActivityKind::ToolCall {
            id: "grep-call".to_string(),
            name: "Grep".to_string(),
            input: serde_json::json!({"pattern": "visible"}),
        },
        TuiSubRunActivityKind::ToolResult {
            tool_call_id: "grep-call".to_string(),
            tool_name: "Grep".to_string(),
            output: "VISIBLE_GREP_RESULT".to_string(),
            content: serde_json::json!({"text": "VISIBLE_GREP_RESULT"}),
            is_error: false,
        },
    ]
    .into_iter()
    .enumerate()
    {
        harness.runtime_event(TuiRuntimeEvent::SubRunActivity(TuiSubRunActivity {
            identity: identity.clone(),
            sequence: sequence as u64 + 1,
            sequence_index: 0,
            kind,
        }));
    }
    harness.render();

    let screen = harness.screen();
    assert!(
        screen.contains("Skill superpowers:using-superpowers"),
        "screen:\n{screen}"
    );
    assert!(screen.contains("VISIBLE_GREP_RESULT"));
    for hidden in ["SKILL_BODY_SENTINEL", "<skill-request>", "system-reminder"] {
        assert!(
            !screen.contains(hidden),
            "screen leaked {hidden}:\n{screen}"
        );
    }
}

#[test]
fn main_with_concurrent_sub_runs_preserves_existing_agent_activity_display() {
    let parent_context = TuiRunContext {
        chat_id: "parent-chat".to_string(),
        run_id: "parent-run".to_string(),
    };
    let mut harness = TuiScenarioHarness::new(120, 40);
    for (index, tool_id) in ["agent-a", "agent-b"].into_iter().enumerate() {
        harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
            context: parent_context.clone(),
            id: tool_id.to_string(),
            provider_id: Some(format!("provider-{tool_id}")),
            name: "Agent".to_string(),
            index,
        });
    }

    for (agent_id, sub_run_id, tool_id, text, sequence) in [
        ("researcher", "child-a", "agent-a", "alpha text", 1),
        ("reviewer", "child-b", "agent-b", "beta thinking", 1),
        ("researcher", "child-a", "agent-a", "alpha output", 2),
    ] {
        let kind = if text.contains("thinking") {
            TuiSubRunActivityKind::Thinking {
                text: text.to_string(),
            }
        } else if text.contains("output") {
            TuiSubRunActivityKind::ToolOutput {
                tool_name: "Bash".to_string(),
                text: text.to_string(),
            }
        } else {
            TuiSubRunActivityKind::Text {
                text: text.to_string(),
            }
        };
        harness.runtime_event(TuiRuntimeEvent::SubRunActivity(TuiSubRunActivity {
            identity: TuiSubRunIdentity {
                agent_id: agent_id.to_string(),
                run_id: crate::tui::model::conversation::interaction::UiRunId::from(sub_run_id),
                parent_chat_id: "parent-chat".to_string(),
                parent_run_id: crate::tui::model::conversation::interaction::UiRunId::from(
                    "parent-run",
                ),
                spawned_by_tool_call_id: tool_id.to_string(),
            },
            sequence,
            sequence_index: 0,
            kind,
        }));
    }
    harness.render();

    let calls = harness
        .app
        .model
        .conversation
        .chats
        .iter()
        .flat_map(|chat| &chat.runs)
        .flat_map(|run| &run.tool_calls)
        .collect::<Vec<_>>();
    let first = calls
        .iter()
        .find(|call| call.id.as_ref().is_some_and(|id| id.as_ref() == "agent-a"))
        .expect("first Agent ToolCall");
    let second = calls
        .iter()
        .find(|call| call.id.as_ref().is_some_and(|id| id.as_ref() == "agent-b"))
        .expect("second Agent ToolCall");
    assert_eq!(
        first
            .activities
            .iter()
            .map(|activity| activity.content.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha text", "alpha output"]
    );
    assert_eq!(
        second
            .activities
            .iter()
            .map(|activity| activity.content.as_str())
            .collect::<Vec<_>>(),
        vec!["beta thinking"]
    );
    let screen = harness.screen();
    assert!(screen.contains("alpha text"), "screen: {screen}");
    assert!(screen.contains("alpha output"), "screen: {screen}");
    assert!(screen.contains("beta thinking"), "screen: {screen}");
}

#[test]
fn cancelled_step_closes_running_tool_and_agent_with_single_terminal_notice() {
    let context = TuiRunContext {
        chat_id: "parent-chat".to_string(),
        run_id: "parent-turn".to_string(),
    };
    let mut harness = TuiScenarioHarness::new(100, 30);

    for (index, name) in ["Bash", "Agent"].into_iter().enumerate() {
        let tool_id = format!("tool-{index}");
        harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
            context: context.clone(),
            id: tool_id.clone(),
            provider_id: Some(format!("provider-{index}")),
            name: name.to_string(),
            index,
        });
        harness.runtime_event(TuiRuntimeEvent::ToolCallStateChanged {
            context: context.clone(),
            id: tool_id,
            provider_id: Some(format!("provider-{index}")),
            name: name.to_string(),
            index,
            arguments: Some(serde_json::json!({"command":"sleep 60"})),
            status: TuiToolCallStatus::Running,
        });
    }

    harness.runtime_event(TuiRuntimeEvent::ToolResult {
        context: context.clone(),
        id: "tool-0".to_string(),
        provider_id: "provider-0".to_string(),
        tool_name: "Bash".to_string(),
        output: "Command cancelled by user".to_string(),
        content: serde_json::json!({"display": "Command cancelled by user"}),
        is_error: true,
        images: Vec::new(),
    });
    harness.runtime_event(TuiRuntimeEvent::RunStep {
        run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
        parent_run_id: None,
        step_id: crate::tui::model::conversation::interaction::UiRunStepId::from("step-1"),
        event: TuiRunStepEvent::Cancelled {
            terminal:
                crate::tui::adapter::tui_runtime_event::TuiRunStepCancellationTerminal::Cancelled,
        },
    });
    harness.runtime_event(TuiRuntimeEvent::Cancelled {
        context: context.clone(),
        duration_ms: 0,
    });
    harness.render();

    let calls = harness
        .app
        .model
        .conversation
        .chats
        .iter()
        .flat_map(|chat| &chat.runs)
        .flat_map(|turn| &turn.tool_calls)
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls
            .iter()
            .find(|call| call.name == "Bash")
            .map(|call| call.status),
        Some(crate::tui::model::conversation::tool_call::ToolCallStatus::Error)
    );
    assert_eq!(
        calls
            .iter()
            .find(|call| call.name == "Agent")
            .map(|call| call.status),
        Some(crate::tui::model::conversation::tool_call::ToolCallStatus::Cancelled)
    );
    let screen = harness.screen();
    assert_eq!(
        screen.matches("✻ Cancelled").count(),
        1,
        "实际屏幕：\n{screen}"
    );
    assert!(
        screen.contains("✗ Run sleep 60"),
        "取消的 Bash gutter 必须显示 ✗，实际屏幕：\n{screen}"
    );
    assert!(
        screen.contains("Command cancelled by user"),
        "取消的 Bash 必须回显命令取消结果，实际屏幕：\n{screen}"
    );
    assert!(
        screen.contains("✗ Agent"),
        "取消的 Agent gutter 必须显示 ✗，实际屏幕：\n{screen}"
    );
    assert!(!screen.contains("Calling tools"), "实际屏幕：\n{screen}");
    assert!(!screen.contains("Completed"), "实际屏幕：\n{screen}");
    harness.assert_idle();
}
