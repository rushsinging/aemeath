use crate::tui::adapter::tui_runtime_event::{
    TuiAgentProgress, TuiAgentProgressKind, TuiRunEvent, TuiRunStepEvent, TuiRuntimeEvent,
    TuiToolCallStatus, TuiTurnContext,
};

use crate::tui::model::output_timeline::OutputTimelineItem;

use super::super::testing::TuiScenarioHarness;

#[test]
fn child_progress_attaches_to_parent_agent_block_without_leaking_into_main_timeline() {
    let parent_context = TuiTurnContext {
        chat_id: "parent-chat".to_string(),
        turn_id: "parent-turn".to_string(),
    };
    let child_context = TuiTurnContext {
        chat_id: "child-chat".to_string(),
        turn_id: "child-turn".to_string(),
    };
    let tool_id = "agent-tool".to_string();
    let marker = "child-private-progress";
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.runtime_event(TuiRuntimeEvent::ToolCallStart {
        context: parent_context.clone(),
        id: tool_id.clone(),
        provider_id: Some("provider-agent-tool".to_string()),
        name: "Agent".to_string(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallUpdate {
        context: parent_context.clone(),
        id: tool_id.clone(),
        provider_id: Some("provider-agent-tool".to_string()),
        name: "Agent".to_string(),
        index: 0,
        arguments_delta: None,
        arguments: Some(serde_json::json!({"role":"coder","prompt":"write hello.rs"})),
        status: TuiToolCallStatus::Ready,
    });
    harness.runtime_event(TuiRuntimeEvent::AgentProgress {
        source_context: child_context.clone(),
        attachment_context: parent_context.clone(),
        tool_id: tool_id.clone(),
        event: TuiAgentProgress {
            sequence: 0,
            kind: TuiAgentProgressKind::Started {
                role: Some("coder".to_string()),
                model: "MiniMax/MiniMax-M3".to_string(),
            },
        },
    });
    harness.runtime_event(TuiRuntimeEvent::AgentProgress {
        source_context: child_context,
        attachment_context: parent_context,
        tool_id,
        event: TuiAgentProgress {
            sequence: 1,
            kind: TuiAgentProgressKind::Message {
                text: marker.to_string(),
            },
        },
    });
    harness.render();

    let agent_call = harness
        .app
        .model
        .conversation
        .chats
        .iter()
        .flat_map(|chat| &chat.turns)
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
fn cancelled_step_closes_running_tool_and_agent_gutters_with_terminal_echo() {
    let context = TuiTurnContext {
        chat_id: "parent-chat".to_string(),
        turn_id: "parent-turn".to_string(),
    };
    let run_id = crate::tui::model::conversation::interaction::UiRunId::from("run-1");
    let step_id = crate::tui::model::conversation::interaction::UiRunStepId::from("step-1");
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.runtime_event(TuiRuntimeEvent::Run {
        run_id: run_id.clone(),
        parent_run_id: None,
        event: TuiRunEvent::Started,
    });
    harness.runtime_event(TuiRuntimeEvent::RunStep {
        run_id: run_id.clone(),
        parent_run_id: None,
        step_id: step_id.clone(),
        event: TuiRunStepEvent::Started,
    });
    for (index, name) in ["Bash", "Agent"].into_iter().enumerate() {
        let tool_id = format!("tool-{index}");
        harness.runtime_event(TuiRuntimeEvent::ToolCallStart {
            context: context.clone(),
            id: tool_id.clone(),
            provider_id: Some(format!("provider-{index}")),
            name: name.to_string(),
            index,
        });
        harness.runtime_event(TuiRuntimeEvent::ToolCallUpdate {
            context: context.clone(),
            id: tool_id,
            provider_id: Some(format!("provider-{index}")),
            name: name.to_string(),
            index,
            arguments_delta: None,
            arguments: Some(serde_json::json!({"command":"sleep 60"})),
            status: TuiToolCallStatus::Running,
        });
    }
    assert_eq!(
        harness
            .app
            .model
            .conversation
            .runtime
            .spinner
            .running_tool_count,
        2
    );

    harness.runtime_event(TuiRuntimeEvent::RunStep {
        run_id,
        parent_run_id: None,
        step_id,
        event: TuiRunStepEvent::Cancelled { confirmed: true },
    });
    harness.render();

    let calls = harness
        .app
        .model
        .conversation
        .chats
        .iter()
        .flat_map(|chat| &chat.turns)
        .flat_map(|turn| &turn.tool_calls)
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|call| {
        call.status == crate::tui::model::conversation::tool_call::ToolCallStatus::Cancelled
    }));
    assert_eq!(
        harness
            .app
            .model
            .conversation
            .runtime
            .spinner
            .running_tool_count,
        0
    );
    let screen = harness.screen();
    assert!(screen.contains("✻ Cancelled"), "实际屏幕：\n{screen}");
    assert!(!screen.contains("Calling tools"), "实际屏幕：\n{screen}");
    harness.assert_idle();
}
