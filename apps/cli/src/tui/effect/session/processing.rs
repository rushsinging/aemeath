mod handle;
mod input_port;
mod logging;

use crate::tui::adapter::event_mapping::{sdk_event_to_tui_event, SdkEventMapping};
use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use std::sync::Arc;

pub(crate) use handle::{shutdown_and_save, ProcessingHandle, SpawnContext, SpawnContextRefs};
pub(crate) use input_port::TuiInputEventPort;
pub(crate) use logging::{log_sdk_event, log_tui_runtime_delivery};

pub(crate) fn spawn_processing(ctx: SpawnContext) -> ProcessingHandle {
    let agent_client = ctx.agent_client.clone();
    let join = composition::delivery_logging::spawn_instrumented(
        composition::delivery_logging::capture(),
        async move {
            let mut stream = match ctx
                .agent_client
                .chat(sdk::ChatRequest {
                    ingress: Arc::new(ctx.input_event_port.clone()),
                })
                .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    let _ = ctx
                        .runtime_tx
                        .send(TuiRuntimeEvent::Error(e.to_string()))
                        .await;
                    let _ = ctx
                        .runtime_tx
                        .send(TuiRuntimeEvent::Done {
                            context: ctx.fallback_context.clone(),
                            duration_ms: None,
                        })
                        .await;
                    return;
                }
            };
            while let Some(event) = stream.recv().await {
                log_sdk_event(&event, "sdk->tui.recv");
                match sdk_event_to_tui_event(event) {
                    SdkEventMapping::Runtime(runtime_event) => {
                        log_tui_runtime_delivery(&runtime_event, "forwarding");
                        if ctx.runtime_tx.send(runtime_event).await.is_err() {
                            crate::tui::log_warn!(
                                "event_delivery boundary=sdk_to_tui kind=runtime_event outcome=receiver_closed"
                            );
                            return;
                        }
                    }
                    SdkEventMapping::Nop => {}
                }
            }
        },
    );
    ProcessingHandle { join, agent_client }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::adapter::tui_runtime_event::TuiRunContext;
    use async_trait::async_trait;
    use sdk::ChatInputEventPort as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_sdk_event_context() -> sdk::ChatEventContext {
        sdk::ChatEventContext::new(
            sdk::ids::ChatId::new("chat-test"),
            sdk::ids::ChatRunId::new("run_step-test"),
        )
    }

    #[test]
    fn production_processing_spawn_is_instrumented_at_creation() {
        let source = include_str!("processing.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("composition::delivery_logging::spawn_instrumented("));
        assert!(production.contains("composition::delivery_logging::capture(),"));
        assert!(!production.contains("tokio::spawn("));
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
    fn sdk_event_to_tui_runtime_event_preserves_model_retry() {
        let event = sdk_event_to_tui_event(sdk::ChatEvent::ModelInvocationRetrying {
            context: test_sdk_event_context(),
            attempt: 2,
            delay: std::time::Duration::from_millis(10_250),
        });

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::ModelInvocationRetrying {
                attempt: 2,
                delay_ms: 10_250,
                ..
            })
        ));
    }

    #[test]
    fn sdk_event_to_tui_runtime_event_maps_token() {
        let event = sdk_event_to_tui_event(sdk::ChatEvent::Token {
            context: test_sdk_event_context(),
            text: "hello".to_string(),
        });

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::Text { text, .. }) if text == "hello"
        ));
    }

    #[test]
    fn sdk_event_to_tui_runtime_event_preserves_agent_progress_identity() {
        let expected_tool_id = sdk::ids::ToolCallId::new("tool-1");
        let event = sdk_event_to_tui_event(sdk::ChatEvent::AgentProgress {
            source_context: sdk::ChatEventContext::new(
                sdk::ids::ChatId::new("child-chat"),
                sdk::ids::ChatRunId::new("child-run_step"),
            ),
            attachment_context: sdk::ChatEventContext::new(
                sdk::ids::ChatId::new("parent-chat"),
                sdk::ids::ChatRunId::new("parent-run_step"),
            ),
            tool_id: expected_tool_id.clone(),
            event: sdk::AgentProgressEventView {
                sequence: 1,
                kind: sdk::AgentProgressKindView::Message {
                    text: "working".to_string(),
                },
            },
        });

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::AgentProgress {
                source_context,
                attachment_context,
                tool_id,
                ..
            }) if source_context.chat_id == sdk::ids::ChatId::new("child-chat").as_str()
                && source_context.run_id == sdk::ids::ChatRunId::new("child-run_step").as_str()
                && attachment_context.chat_id == sdk::ids::ChatId::new("parent-chat").as_str()
                && attachment_context.run_id == sdk::ids::ChatRunId::new("parent-run_step").as_str()
                && tool_id == expected_tool_id.as_str()
        ));
    }

    #[test]
    fn sdk_event_to_tui_runtime_event_preserves_tool_progress_identity() {
        let expected_chat = sdk::ids::ChatId::new("chat-1");
        let expected_run = sdk::ids::ChatRunId::new("run-1");
        let expected_tool_id = sdk::ids::ToolCallId::new("bash-1");
        let event = sdk_event_to_tui_event(sdk::ChatEvent::ToolProgress {
            context: sdk::ChatEventContext::new(expected_chat.clone(), expected_run.clone()),
            tool_id: expected_tool_id.clone(),
            event: sdk::ToolProgressEventView {
                text: "stdout line\n".to_string(),
            },
        });

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::ToolProgress {
                context,
                tool_id,
                event,
            }) if context.chat_id == expected_chat.as_str()
                && context.run_id == expected_run.as_str()
                && tool_id == expected_tool_id.as_str()
                && event.text == "stdout line\n"
        ));
    }

    #[test]
    fn sdk_event_to_tui_runtime_event_maps_compact_finished() {
        let event = sdk_event_to_tui_event(sdk::ChatEvent::CompactFinished {
            messages: vec![sdk::ChatMessage::user_text("hello")],
            notice: "✓ 上下文压缩完成".to_string(),
        });

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::CompactFinished { messages, notice })
                if messages[0].text_content() == "hello" && notice == "✓ 上下文压缩完成"
        ));
    }

    #[test]
    fn sdk_event_to_tui_runtime_event_maps_working_directory_changed() {
        let event = sdk_event_to_tui_event(sdk::ChatEvent::WorkingDirectoryChanged {
            path_base: "/tmp".to_string(),
            workspace_root: "/tmp".to_string(),
            workspace: sdk::WorkspaceContextView {
                path_base: "/tmp".into(),
                workspace_root: "/tmp".into(),
                context_stack: Vec::new(),
            },
        });

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::WorkspaceSnapshot(snapshot))
                if snapshot.path_base == "/tmp" && snapshot.workspace_root == "/tmp"
        ));
    }

    #[test]
    fn sdk_event_to_tui_runtime_event_maps_tasks_snapshot() {
        let event = sdk_event_to_tui_event(sdk::ChatEvent::TasksSnapshot {
            tasks: Box::new(sdk::TaskStatusView {
                lines: vec!["[ ] #1 task".to_string()],
            }),
        });

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::TasksSnapshot { lines })
                if lines == vec!["[ ] #1 task".to_string()]
        ));
    }

    #[tokio::test]
    async fn processing_handle_cancels_current_run_without_observing_run_started() {
        #[derive(Default)]
        struct RecordingCancelClient {
            cancel_current_called: std::sync::atomic::AtomicUsize,
        }

        #[async_trait]
        impl sdk::AgentClient for RecordingCancelClient {
            fn cancel_current_run(
                &self,
                _deadline: sdk::ControlDeadline,
            ) -> sdk::CancelCurrentRunOutcome {
                let count = self
                    .cancel_current_called
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if count == 0 {
                    sdk::CancelCurrentRunOutcome::Accepted
                } else {
                    sdk::CancelCurrentRunOutcome::AlreadyCancelling
                }
            }

            async fn chat(
                &self,
                _input: sdk::ChatRequest,
            ) -> Result<sdk::ChatStream, sdk::SdkError> {
                unreachable!()
            }
        }

        let client = Arc::new(RecordingCancelClient::default());
        let handle = ProcessingHandle {
            join: tokio::spawn(async {}),
            agent_client: client.clone(),
        };

        assert_eq!(
            handle.cancel_current_run(),
            sdk::CancelCurrentRunOutcome::Accepted
        );
        assert_eq!(
            handle.cancel_current_run(),
            sdk::CancelCurrentRunOutcome::AlreadyCancelling
        );
        assert_eq!(
            client
                .cancel_current_called
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );
    }

    #[tokio::test]
    async fn spawn_processing_propagates_captured_context() {
        let (runtime_tx, _runtime_rx) = tokio::sync::mpsc::channel(16);
        let (local_tx, _local_rx) = tokio::sync::mpsc::channel(16);
        let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
        let client = Arc::new(ContextCapturingAgentClient::new(observed_tx));
        let (_input_tx, input_port) = TuiInputEventPort::channel();
        let expected = composition::delivery_logging::LogContext {
            session_id: Some("processing-session".to_string()),
            ..composition::delivery_logging::LogContext::default()
        };

        composition::delivery_logging::instrument(expected.clone(), async move {
            spawn_processing(SpawnContext {
                runtime_tx,
                local_tx,
                input_event_port: input_port,
                agent_client: client,
                fallback_context: TuiRunContext {
                    chat_id: "fallback-chat".to_string(),
                    run_id: "fallback-run_step".to_string(),
                },
            });
        })
        .await;

        assert_eq!(observed_rx.await.unwrap(), expected);
    }

    struct ContextCapturingAgentClient {
        observed: std::sync::Mutex<
            Option<tokio::sync::oneshot::Sender<composition::delivery_logging::LogContext>>,
        >,
    }

    impl ContextCapturingAgentClient {
        fn new(
            observed: tokio::sync::oneshot::Sender<composition::delivery_logging::LogContext>,
        ) -> Self {
            Self {
                observed: std::sync::Mutex::new(Some(observed)),
            }
        }
    }

    #[async_trait]
    impl sdk::AgentClient for ContextCapturingAgentClient {
        async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
            if let Some(tx) = self.observed.lock().unwrap().take() {
                let _ = tx.send(composition::delivery_logging::capture());
            }
            let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
            Ok(sdk::ChatStream::new(rx))
        }
    }

    #[tokio::test]
    async fn test_spawn_processing_done_emits_done_event() {
        let (runtime_tx, mut runtime_rx) = tokio::sync::mpsc::channel(16);
        let (local_tx, _local_rx) = tokio::sync::mpsc::channel(16);
        let client = Arc::new(DoneOnlyAgentClient::default());

        let (_input_tx, input_port) = TuiInputEventPort::channel();
        spawn_processing(SpawnContext {
            runtime_tx,
            local_tx,
            input_event_port: input_port,
            agent_client: client.clone(),
            fallback_context: TuiRunContext {
                chat_id: "fallback-chat".to_string(),
                run_id: "fallback-run_step".to_string(),
            },
        });

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), runtime_rx.recv())
            .await
            .expect("Done event should be forwarded")
            .expect("runtime channel should receive Done");
        assert!(matches!(
            event,
            TuiRuntimeEvent::Done { context, .. }
                if context.chat_id == sdk::ids::ChatId::new("chat-test").as_str()
                    && context.run_id == sdk::ids::ChatRunId::new("run_step-test").as_str()
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
