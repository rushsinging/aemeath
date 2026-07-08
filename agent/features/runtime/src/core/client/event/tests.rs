use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::convert::{
    agent_progress_event_to_sdk, runtime_event_to_sdk_event, runtime_hook_event_to_sdk,
};
use super::RuntimeQueueDrainPort;
use crate::business::chat::looping::RuntimeTurnContext;
use crate::business::chat::{RuntimeHookEvent, RuntimeHookEventStatus};
use sdk::{AgentProgressKindView, ChangeSet, ChatEvent, HookEventStatus};

struct CountingQueueDrainPort {
    calls: Arc<AtomicUsize>,
    queued: Mutex<Option<Vec<String>>>,
}

impl CountingQueueDrainPort {
    fn new(queued: Option<Vec<String>>) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            queued: Mutex::new(queued),
        }
    }
}

impl sdk::QueueDrainPort for CountingQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> sdk::QueueFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.queued.lock().unwrap().take()
        })
    }
}

#[test]
fn test_runtime_tasks_snapshot_emits_sdk_event() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();
    let (change_tx, _change_rx) = tokio::sync::watch::channel(ChangeSet::empty());

    let view = sdk::TaskStatusView {
        lines: vec!["[ ] #1 sample".to_string()],
    };
    let event = runtime_event_to_sdk_event(
        crate::business::chat::RuntimeStreamEvent::TasksSnapshot {
            tasks: Box::new(view.clone()),
        },
        &change_tx,
    );

    match event {
        ChatEvent::TasksSnapshot { tasks } => assert_eq!(tasks.lines, view.lines),
        other => panic!("unexpected event: {other:?}"),
    }
    drop(tx);
}

#[tokio::test]
async fn test_runtime_queue_drain_port_forwards_to_sdk_queue() {
    let sdk_queue = Arc::new(CountingQueueDrainPort::new(Some(vec![
        "queued input".to_string()
    ])));
    let calls = sdk_queue.calls.clone();
    let queue = RuntimeQueueDrainPort::new(Some(sdk_queue));

    let drained = crate::business::chat::QueueDrainPort::drain_queued_input(&queue).await;

    assert_eq!(drained, Some(vec!["queued input".to_string()]));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_runtime_queue_drain_port_without_sdk_queue_returns_none() {
    let queue = RuntimeQueueDrainPort::new(None);

    let drained = crate::business::chat::QueueDrainPort::drain_queued_input(&queue).await;

    assert_eq!(drained, None);
}

// issue #646：share → sdk view 转发测试
#[test]
fn test_agent_progress_event_to_sdk_started_with_role() {
    let ev = share::tool::AgentProgressEvent {
        sequence: 7,
        kind: share::tool::AgentProgressKind::Started {
            role: Some("coder".into()),
            model: "Zhipu/glm-5.2".into(),
        },
    };
    let view = agent_progress_event_to_sdk(ev);
    assert_eq!(view.sequence, 7);
    match view.kind {
        AgentProgressKindView::Started { role, model } => {
            assert_eq!(role.as_deref(), Some("coder"));
            assert_eq!(model, "Zhipu/glm-5.2");
        }
        _ => panic!("expected Started"),
    }
}

#[test]
fn test_agent_progress_event_to_sdk_started_without_role() {
    let ev = share::tool::AgentProgressEvent {
        sequence: 0,
        kind: share::tool::AgentProgressKind::Started {
            role: None,
            model: "default-model".into(),
        },
    };
    let view = agent_progress_event_to_sdk(ev);
    match view.kind {
        AgentProgressKindView::Started { role, model } => {
            assert!(role.is_none());
            assert_eq!(model, "default-model");
        }
        _ => panic!("expected Started"),
    }
}

#[test]
fn test_agent_progress_event_to_sdk_started_preserves_sequence() {
    // 确保 sequence 字段也透传
    let ev = share::tool::AgentProgressEvent {
        sequence: 42,
        kind: share::tool::AgentProgressKind::Started {
            role: None,
            model: "m".into(),
        },
    };
    let view = agent_progress_event_to_sdk(ev);
    assert_eq!(view.sequence, 42);
}

fn test_turn_context() -> RuntimeTurnContext {
    RuntimeTurnContext::new(
        sdk::ids::ChatId::new("chat-test"),
        sdk::ids::ChatTurnId::new("turn-test"),
    )
}

fn event_mapping_context() -> tokio::sync::watch::Sender<ChangeSet> {
    let (change_tx, _change_rx) = tokio::sync::watch::channel(ChangeSet::empty());
    change_tx
}

#[test]
fn test_runtime_text_maps_to_sdk_token_with_context() {
    let change_tx = event_mapping_context();
    let event = runtime_event_to_sdk_event(
        crate::business::chat::RuntimeStreamEvent::Text {
            context: test_turn_context(),
            text: "hello".to_string(),
        },
        &change_tx,
    );

    match event {
        ChatEvent::Token { context, text } => {
            assert_eq!(context.chat_id, sdk::ids::ChatId::new("chat-test"));
            assert_eq!(context.turn_id, sdk::ids::ChatTurnId::new("turn-test"));
            assert_eq!(text, "hello");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_runtime_tool_call_start_preserves_provider_and_index() {
    let change_tx = event_mapping_context();
    let tool_id = sdk::ids::ToolCallId::new("tool-1");
    let event = runtime_event_to_sdk_event(
        crate::business::chat::RuntimeStreamEvent::ToolCallStart {
            context: test_turn_context(),
            id: tool_id.clone(),
            provider_id: Some("provider-1".to_string()),
            name: "Read".to_string(),
            index: 3,
        },
        &change_tx,
    );

    match event {
        ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => {
            assert_eq!(context.chat_id, sdk::ids::ChatId::new("chat-test"));
            assert_eq!(id, tool_id);
            assert_eq!(provider_id.as_deref(), Some("provider-1"));
            assert_eq!(name, "Read");
            assert_eq!(index, 3);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_runtime_tool_result_maps_images_and_content() {
    let change_tx = event_mapping_context();
    let event = runtime_event_to_sdk_event(
        crate::business::chat::RuntimeStreamEvent::ToolResult {
            context: test_turn_context(),
            id: sdk::ids::ToolCallId::new("tool-1"),
            provider_id: "provider-1".to_string(),
            tool_name: "Read".to_string(),
            output: "ok".to_string(),
            content: serde_json::json!({"line_count": 1}),
            is_error: false,
            images: vec![share::tool::ImageData {
                base64: "abc".to_string(),
                media_type: "image/png".to_string(),
            }],
        },
        &change_tx,
    );

    match event {
        ChatEvent::ToolResult {
            tool_name,
            output,
            content,
            is_error,
            images,
            ..
        } => {
            assert_eq!(tool_name, "Read");
            assert_eq!(output, "ok");
            assert_eq!(content, serde_json::json!({"line_count": 1}));
            assert!(!is_error);
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].base64, "abc");
            assert_eq!(images[0].media_type, "image/png");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_runtime_working_directory_changed_marks_project_changeset() {
    let change_tx = event_mapping_context();
    let workspace = crate::business::session::PersistedWorkspaceContext {
        path_base: "/tmp/project".to_string(),
        workspace_root: "/tmp/project".to_string(),
        context_stack: Vec::new(),
    };
    let event = runtime_event_to_sdk_event(
        crate::business::chat::RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base: "/tmp/project".to_string(),
            workspace_root: "/tmp/project".to_string(),
            workspace: workspace.clone(),
        },
        &change_tx,
    );

    assert!(change_tx.borrow().contains(ChangeSet::PROJECT));
    match event {
        ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => {
            assert_eq!(path_base, "/tmp/project");
            assert_eq!(workspace_root, "/tmp/project");
            assert_eq!(
                workspace.path_base,
                std::path::PathBuf::from("/tmp/project")
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_runtime_user_messages_adopted_injects_input_ids() {
    let change_tx = event_mapping_context();
    let accepted_id = sdk::InputId::new_v7();
    let queued_id = sdk::InputId::new_v7();
    let event = runtime_event_to_sdk_event(
        crate::business::chat::RuntimeStreamEvent::UserMessagesAdopted {
            items: vec![(
                accepted_id.clone(),
                share::message::Message::user("accepted"),
            )],
            queued: vec![(queued_id.clone(), share::message::Message::user("queued"))],
        },
        &change_tx,
    );

    match event {
        ChatEvent::UserMessagesAdopted { items, queued } => {
            assert_eq!(items.len(), 1);
            assert_eq!(queued.len(), 1);
            assert_eq!(items[0].input_id, Some(accepted_id));
            assert_eq!(queued[0].input_id, Some(queued_id));
            assert_eq!(items[0].text_content(), "accepted");
            assert_eq!(queued[0].text_content(), "queued");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_runtime_hook_event_maps_status_and_result() {
    let view = runtime_hook_event_to_sdk(RuntimeHookEvent {
        hook_name: "Stop".to_string(),
        status: RuntimeHookEventStatus::Blocked,
        matcher: Some("matcher".to_string()),
        command: Some("cmd".to_string()),
        result: Some(crate::business::chat::RuntimeHookExecutionResult {
            exit_code: Some(1),
            stdout: "out".to_string(),
            stderr: "err".to_string(),
            decision: Some("block".to_string()),
            reason: Some("no".to_string()),
            additional_context: Some("ctx".to_string()),
        }),
    });

    assert_eq!(view.hook_name, "Stop");
    assert_eq!(view.status, HookEventStatus::Blocked);
    assert_eq!(view.matcher.as_deref(), Some("matcher"));
    let result = view.result.expect("hook result should map");
    assert_eq!(result.exit_code, Some(1));
    assert_eq!(result.stdout, "out");
    assert_eq!(result.stderr, "err");
    assert_eq!(result.decision.as_deref(), Some("block"));
    assert_eq!(result.reason.as_deref(), Some("no"));
}
