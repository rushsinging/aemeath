mod event_mapping;
mod handle;
mod input_port;
mod logging;

use crate::tui::adapter::event_mapping::{sdk_event_to_tui_event, SdkEventMapping};
use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use std::sync::Arc;

pub(crate) use handle::{
    shutdown_and_save, ProcessingHandle, RunCancelState, SpawnContext, SpawnContextRefs,
};
pub(crate) use input_port::TuiInputEventPort;
pub(crate) use logging::log_sdk_event;

pub(crate) fn spawn_processing(ctx: SpawnContext) -> ProcessingHandle {
    let run_cancel_state = Arc::new(std::sync::Mutex::new(RunCancelState::Idle));
    let run_cancel_state_for_task = run_cancel_state.clone();
    let agent_client = ctx.agent_client.clone();
    let agent_client_for_task = agent_client.clone();
    let join = composition::delivery_logging::spawn_instrumented(
        composition::delivery_logging::capture(),
        async move {
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
                match &event {
                    sdk::ChatEvent::RunStarted { run_id, .. } => {
                        let cancel_requested = {
                            let mut state = run_cancel_state_for_task
                                .lock()
                                .unwrap_or_else(|error| error.into_inner());
                            let requested = matches!(
                                &*state,
                                RunCancelState::AwaitingStart {
                                    cancel_requested: true
                                }
                            );
                            *state = RunCancelState::Active(run_id.clone());
                            requested
                        };
                        if cancel_requested {
                            let _ = agent_client_for_task.cancel_run(run_id);
                        }
                    }
                    sdk::ChatEvent::RunCancelled { run_id } => {
                        let mut state = run_cancel_state_for_task
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        if matches!(&*state, RunCancelState::Active(active) if active == run_id) {
                            *state = RunCancelState::Idle;
                        }
                    }
                    sdk::ChatEvent::Done { .. } | sdk::ChatEvent::DoneWithDurationMs { .. } => {
                        *run_cancel_state_for_task
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()) = RunCancelState::Idle;
                    }
                    _ => {}
                }
                log_sdk_event(&event, "sdk->tui.recv");
                match sdk_event_to_tui_event(event) {
                    SdkEventMapping::Runtime(runtime_event) => {
                        if ctx.runtime_tx.send(runtime_event).await.is_err() {
                            return;
                        }
                    }
                    SdkEventMapping::Nop => {}
                }
            }
        },
    );
    ProcessingHandle {
        join,
        agent_client,
        run_cancel_state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::adapter::tui_runtime_event::TuiTurnContext;
    use async_trait::async_trait;
    use sdk::ChatInputEventPort as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_sdk_event_context() -> sdk::ChatEventContext {
        sdk::ChatEventContext::new(
            sdk::ids::ChatId::new("chat-test"),
            sdk::ids::ChatTurnId::new("turn-test"),
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

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::AgentProgress {
                context,
                tool_id,
                ..
            }) if context.chat_id == sdk::ids::ChatId::new("chat-progress").as_str()
                && context.turn_id == sdk::ids::ChatTurnId::new("turn-progress").as_str()
                && tool_id == expected_tool_id.as_str()
        ));
    }

    #[test]
    fn sdk_event_to_tui_runtime_event_preserves_hook_message() {
        let event = sdk_event_to_tui_event(sdk::ChatEvent::HookMessage(sdk::HookMessageView {
            point: "PreToolUse".to_string(),
            source: "matcher:Bash".to_string(),
            execution_ordinal: 2,
            attempt: 3,
            kind: sdk::HookMessageKindView::AdditionalContext,
            text: "Use formatter".to_string(),
        }));

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::HookMessage(message))
                if message.point == "PreToolUse" && message.text == "Use formatter"
        ));
    }

    #[test]
    fn sdk_event_to_tui_runtime_event_maps_compact_finished() {
        let event = sdk_event_to_tui_event(sdk::ChatEvent::CompactFinished {
            messages: vec![sdk::ChatMessage::user_text("hello")],
        });

        assert!(matches!(
            event,
            SdkEventMapping::Runtime(TuiRuntimeEvent::CompactFinished { messages })
                if messages[0].text_content() == "hello"
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
    async fn test_processing_handle_cancels_the_active_run_id_synchronously() {
        #[derive(Default)]
        struct RecordingCancelClient {
            cancel_step_called: std::sync::atomic::AtomicUsize,
        }

        #[async_trait]
        impl sdk::AgentClient for RecordingCancelClient {
            fn cancel_run(&self, _run_id: &sdk::RunId) -> sdk::CancelRunOutcome {
                unreachable!("should use cancel_run_step")
            }

            fn cancel_run_step(
                &self,
                _run_id: &sdk::RunId,
                _step_id: Option<&sdk::RunStepId>,
                _deadline: sdk::ControlDeadline,
            ) -> sdk::CancelRunStepOutcome {
                let count = self
                    .cancel_step_called
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if count == 0 {
                    sdk::CancelRunStepOutcome::Accepted
                } else {
                    sdk::CancelRunStepOutcome::AlreadyCancelling
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
        let run_id = sdk::RunId::new_v7();
        let handle = ProcessingHandle {
            join: tokio::spawn(async {}),
            agent_client: client.clone(),
            run_cancel_state: Arc::new(std::sync::Mutex::new(RunCancelState::Active(
                run_id.clone(),
            ))),
        };

        assert_eq!(handle.cancel_current_run(), sdk::CancelRunOutcome::Accepted);
        assert_eq!(
            client
                .cancel_step_called
                .load(std::sync::atomic::Ordering::SeqCst),
            1,
            "cancel_current_run must call cancel_run_step"
        );

        // Second call: cancel_run_step returns AlreadyCancelling -> AlreadyCancelling
        assert_eq!(
            handle.cancel_current_run(),
            sdk::CancelRunOutcome::AlreadyCancelling
        );
        assert_eq!(
            client
                .cancel_step_called
                .load(std::sync::atomic::Ordering::SeqCst),
            2,
            "second cancel_current_run must call cancel_run_step again"
        );
    }

    #[tokio::test]
    async fn test_processing_handle_idle_cancel_does_not_arm_next_run() {
        let client = Arc::new(DoneOnlyAgentClient::default());
        let run_cancel_state = Arc::new(std::sync::Mutex::new(RunCancelState::Idle));
        let handle = ProcessingHandle {
            join: tokio::spawn(async {}),
            agent_client: client,
            run_cancel_state: run_cancel_state.clone(),
        };

        assert_eq!(handle.cancel_current_run(), sdk::CancelRunOutcome::NotFound);
        assert!(matches!(
            &*run_cancel_state.lock().unwrap(),
            RunCancelState::Idle
        ));
    }

    #[tokio::test]
    async fn test_processing_handle_buffers_cancel_before_run_started() {
        let client = Arc::new(DoneOnlyAgentClient::default());
        let run_cancel_state = Arc::new(std::sync::Mutex::new(RunCancelState::AwaitingStart {
            cancel_requested: false,
        }));
        let handle = ProcessingHandle {
            join: tokio::spawn(async {}),
            agent_client: client,
            run_cancel_state: run_cancel_state.clone(),
        };

        assert_eq!(handle.cancel_current_run(), sdk::CancelRunOutcome::Accepted);
        assert!(matches!(
            &*run_cancel_state.lock().unwrap(),
            RunCancelState::AwaitingStart {
                cancel_requested: true
            }
        ));
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
                fallback_context: TuiTurnContext {
                    chat_id: "fallback-chat".to_string(),
                    turn_id: "fallback-turn".to_string(),
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
        fn cancel_run(&self, _run_id: &sdk::RunId) -> sdk::CancelRunOutcome {
            sdk::CancelRunOutcome::NotFound
        }

        fn cancel_run_step(
            &self,
            _run_id: &sdk::RunId,
            _step_id: Option<&sdk::RunStepId>,
            _deadline: sdk::ControlDeadline,
        ) -> sdk::CancelRunStepOutcome {
            sdk::CancelRunStepOutcome::NotFound
        }

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
            fallback_context: TuiTurnContext {
                chat_id: "fallback-chat".to_string(),
                turn_id: "fallback-turn".to_string(),
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
                    && context.turn_id == sdk::ids::ChatTurnId::new("turn-test").as_str()
        ));
        assert_eq!(client.sync_calls.load(Ordering::SeqCst), 0);
    }

    #[derive(Default)]
    struct DoneOnlyAgentClient {
        sync_calls: AtomicUsize,
    }

    #[async_trait]
    impl sdk::AgentClient for DoneOnlyAgentClient {
        fn cancel_run(&self, _run_id: &sdk::RunId) -> sdk::CancelRunOutcome {
            sdk::CancelRunOutcome::NotFound
        }

        fn cancel_run_step(
            &self,
            _run_id: &sdk::RunId,
            _step_id: Option<&sdk::RunStepId>,
            _deadline: sdk::ControlDeadline,
        ) -> sdk::CancelRunStepOutcome {
            sdk::CancelRunStepOutcome::NotFound
        }

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
