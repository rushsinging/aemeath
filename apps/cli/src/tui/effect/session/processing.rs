mod event_mapping;
mod handle;
mod input_port;
mod logging;

use crate::tui::app::event::UiEvent;
use std::sync::Arc;

pub(crate) use event_mapping::sdk_event_to_ui_event;
pub(crate) use handle::{shutdown_and_save, ProcessingHandle, SpawnContext, SpawnContextRefs};
pub(crate) use input_port::TuiInputEventPort;
pub(crate) use logging::log_sdk_event;

use logging::log_ui_tool_event;

pub(crate) fn spawn_processing(ctx: SpawnContext) -> ProcessingHandle {
    // #639：cancel 句柄共享 slot。chat() 在 task 内部 await，返回 stream 后回填此 slot；
    // TUI 侧 ProcessingHandle::cancel() 读取触发。cancel-before-chat-returns 的极小窗口内
    // 为 no-op（chat() 微秒级返回），可接受。
    let cancel_slot: Arc<std::sync::Mutex<Option<sdk::CancelHandle>>> =
        Arc::new(std::sync::Mutex::new(None));
    let cancel_slot_for_task = cancel_slot.clone();
    let join = tokio::spawn(async move {
        let mut stream = match ctx
            .agent_client
            .chat(sdk::ChatRequest {
                user_input: None,
                // 文本队列已断开（#390 A3）：统一走 input_events 事件通道。
                queue_drain: None,
                input_events: Some(Arc::new(ctx.input_event_port.clone())),
            })
            .await
        {
            Ok(stream) => stream,
            Err(e) => {
                let _ = ctx.tx.send(UiEvent::Error(e.to_string())).await;
                let _ = ctx
                    .tx
                    .send(UiEvent::Done {
                        context: ctx.fallback_context.clone(),
                    })
                    .await;
                return;
            }
        };
        // 回填 cancel 句柄，供 Ctrl+C/Esc 即时中断。
        if let Ok(mut guard) = cancel_slot_for_task.lock() {
            *guard = Some(stream.cancel_handle());
        }
        while let Some(event) = stream.recv().await {
            log_sdk_event(&event, "sdk->ui.recv");
            let ui_event = sdk_event_to_ui_event(event);
            log_ui_tool_event(&ui_event, "sdk->ui.mapped");
            if ctx.tx.send(ui_event).await.is_err() {
                return;
            }
        }
    });
    ProcessingHandle {
        join,
        cancel: cancel_slot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::event::UiTurnContext;
    use async_trait::async_trait;
    use sdk::ChatInputEventPort as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_sdk_event_context() -> sdk::ChatEventContext {
        sdk::ChatEventContext::new(
            sdk::ids::ChatId::new("chat-test"),
            sdk::ids::ChatTurnId::new("turn-test"),
        )
    }

    #[tokio::test]
    async fn test_tui_input_port_recv_next_and_close() {
        let (tx, port) = TuiInputEventPort::channel();
        tx.send(sdk::ChatInputEvent::UserMessage {
            id: sdk::InputId::new_v7(),
            text: "x".into(),
            images: vec![],
        })
        .unwrap();
        assert!(port.recv_next().await.is_some());
        drop(tx);
        assert!(port.recv_next().await.is_none());
    }

    #[test]
    fn test_sdk_event_to_ui_event_maps_token() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::Token {
            context: test_sdk_event_context(),
            text: "hello".to_string(),
        });

        match event {
            UiEvent::Text { text, .. } => assert_eq!(text, "hello"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn test_sdk_event_to_ui_event_preserves_agent_progress_context() {
        let expected_tool_id = sdk::ids::ToolCallId::new("tool-1");
        let event = sdk_event_to_ui_event(sdk::ChatEvent::AgentProgress {
            context: sdk::ChatEventContext::new(
                sdk::ids::ChatId::new("chat-progress"),
                sdk::ids::ChatTurnId::new("turn-progress"),
            ),
            tool_id: expected_tool_id.clone(),
            event: sdk::AgentProgressEventView {
                sequence: 1,
                kind: sdk::AgentProgressKindView::Message {
                    text: "working".to_string(),
                },
            },
        });

        match event {
            UiEvent::AgentProgress {
                context, tool_id, ..
            } => {
                assert_eq!(
                    context.chat_id,
                    crate::tui::model::conversation::ids::ChatId::new("chat-progress")
                );
                assert_eq!(
                    context.turn_id,
                    crate::tui::model::conversation::ids::ChatTurnId::new("turn-progress")
                );
                assert_eq!(tool_id, expected_tool_id);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn test_sdk_event_to_ui_event_maps_compact_finished() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::CompactFinished {
            messages: vec![sdk::ChatMessage::user_text("hello")],
        });

        match event {
            UiEvent::CompactFinished { messages } => {
                assert_eq!(messages[0].text_content(), "hello")
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn test_sdk_event_to_ui_event_maps_working_directory_changed() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::WorkingDirectoryChanged {
            path_base: "/tmp".to_string(),
            workspace_root: "/tmp".to_string(),
            workspace: sdk::WorkspaceContextView {
                path_base: "/tmp".into(),
                workspace_root: "/tmp".into(),
                context_stack: Vec::new(),
            },
        });

        match event {
            UiEvent::WorkingDirectoryChanged(update) => {
                assert_eq!(update.raw_path_base, std::path::PathBuf::from("/tmp"));
                assert_eq!(update.workspace.path_base, std::path::PathBuf::from("/tmp"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn test_sdk_event_to_ui_event_maps_tasks_snapshot() {
        let view = sdk::TaskStatusView {
            lines: vec!["[ ] #1 task".to_string()],
        };
        let event = sdk_event_to_ui_event(sdk::ChatEvent::TasksSnapshot {
            tasks: Box::new(view.clone()),
        });

        match event {
            UiEvent::TaskStatusChanged(v) => assert_eq!(v.lines, view.lines),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_spawn_processing_done_emits_done_event() {
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::channel(16);
        let client = Arc::new(DoneOnlyAgentClient::default());

        let (_input_tx, input_port) = TuiInputEventPort::channel();
        spawn_processing(SpawnContext {
            tx: ui_tx,
            input_event_port: input_port,
            agent_client: client.clone(),
            fallback_context: UiTurnContext {
                chat_id: crate::tui::model::conversation::ids::ChatId::new("fallback-chat"),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("fallback-turn"),
            },
        });

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), ui_rx.recv())
            .await
            .expect("Done event should be forwarded")
            .expect("ui channel should receive Done");
        let expected_chat = crate::tui::model::conversation::ids::ChatId::new("chat-test");
        let expected_turn = crate::tui::model::conversation::ids::ChatTurnId::new("turn-test");
        assert!(matches!(
            event,
            UiEvent::Done { context }
                if context.chat_id == expected_chat && context.turn_id == expected_turn
        ));
        assert_eq!(client.sync_calls.load(Ordering::SeqCst), 0);
    }

    #[derive(Default)]
    struct DoneOnlyAgentClient {
        sync_calls: AtomicUsize,
    }

    #[async_trait]
    impl sdk::AgentClient for DoneOnlyAgentClient {
        async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tx.send(sdk::ChatEvent::Done {
                context: test_sdk_event_context(),
            })
            .unwrap();
            drop(tx);
            Ok(sdk::ChatStream::new(rx))
        }
    }
}
