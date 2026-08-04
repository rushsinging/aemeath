use crate::tui::adapter::tui_runtime_event::{
    TuiAgentProgress, TuiAgentProgressKind, TuiRunContext, TuiRunStepEvent, TuiRuntimeEvent,
    TuiToolCallStatus,
};

use crate::tui::model::output_timeline::OutputTimelineItem;

use super::super::testing::TuiScenarioHarness;

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
fn cancelled_step_closes_running_tool_and_agent_with_single_terminal_notice() {
    let context = TuiRunContext {
        chat_id: "parent-chat".to_string(),
        run_id: "parent-turn".to_string(),
    };
    let mut harness = TuiScenarioHarness::new(100, 30);

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
        event: TuiRunStepEvent::Cancelled { confirmed: true },
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
