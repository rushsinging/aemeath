use crate::tui::app::event::{StatusContextUpdate, UiEvent, UiTurnContext};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub(crate) struct TuiInputEventPort {
    rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<sdk::ChatInputEvent>>>,
}

impl TuiInputEventPort {
    pub(crate) fn channel() -> (mpsc::UnboundedSender<sdk::ChatInputEvent>, Self) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            tx,
            Self {
                rx: Arc::new(tokio::sync::Mutex::new(rx)),
            },
        )
    }
}

impl sdk::ChatInputEventPort for TuiInputEventPort {
    fn recv_next<'a>(&'a self) -> sdk::InputEventOptFuture<'a> {
        Box::pin(async move { self.rx.lock().await.recv().await })
    }

    fn drain_input_events<'a>(&'a self) -> sdk::InputEventFuture<'a> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        })
    }
}

pub(crate) fn sdk_event_to_ui_event(event: sdk::ChatEvent) -> UiEvent {
    match event {
        sdk::ChatEvent::Token { context, text } => UiEvent::Text {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::Thinking { context, text } => UiEvent::Thinking {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::BlockComplete { context, text } => UiEvent::BlockComplete {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => UiEvent::ToolCallStart {
            context: context.into(),
            id,
            provider_id,
            name,
            index,
        },
        sdk::ChatEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => UiEvent::ToolCallUpdate {
            context: context.into(),
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        },
        sdk::ChatEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
            ..
        } => UiEvent::ToolResult {
            context: context.into(),
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        },
        sdk::ChatEvent::SystemMessage(msg) => UiEvent::SystemMessage(msg),
        sdk::ChatEvent::Error(msg) => UiEvent::Error(msg),
        sdk::ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => UiEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        sdk::ChatEvent::MessagesSync(messages) => UiEvent::MessagesSync(messages),
        sdk::ChatEvent::UserMessagesAdded { items } => UiEvent::UserMessagesAdded(items),
        sdk::ChatEvent::Done { context } => UiEvent::Done {
            context: context.into(),
        },
        sdk::ChatEvent::DoneWithDurationMs {
            context,
            duration_ms,
        } => UiEvent::DoneWithDuration {
            context: context.into(),
            duration: std::time::Duration::from_millis(duration_ms),
        },
        sdk::ChatEvent::Cancelled { context } => UiEvent::Cancelled {
            context: context.into(),
        },
        sdk::ChatEvent::LiveTps(tps) => UiEvent::LiveTps(tps),
        sdk::ChatEvent::CurrentTurnChanged(turn) | sdk::ChatEvent::TurnChanged(turn) => {
            UiEvent::CurrentTurnChanged(turn)
        }
        sdk::ChatEvent::HookEvent(event) => UiEvent::HookEvent(event),
        sdk::ChatEvent::AskUserBatch { items, reply_tx } => {
            UiEvent::AskUserBatch { items, reply_tx }
        }
        sdk::ChatEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => UiEvent::AgentProgress {
            context: context.into(),
            tool_id,
            event,
        },
        sdk::ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
            path_base: crate::tui::app::display_status_path(std::path::Path::new(&path_base)),
            workspace_root: crate::tui::app::display_status_path(std::path::Path::new(
                &workspace_root,
            )),
            branch: crate::tui::app::git_branch_for(std::path::Path::new(&workspace_root)),
            kind: crate::tui::app::worktree_kind_for(std::path::Path::new(&workspace_root)),
            raw_path_base: std::path::PathBuf::from(path_base),
            raw_workspace_root: std::path::PathBuf::from(workspace_root),
            workspace,
        }),
        sdk::ChatEvent::TasksChanged => UiEvent::TaskStatusChanged,
        sdk::ChatEvent::ConfigReloaded { changed_keys } => {
            let keys_str = changed_keys.join(", ");
            UiEvent::SystemMessage(format!("[config reloaded] changed: {}", keys_str))
        }
        sdk::ChatEvent::SessionReset => UiEvent::SessionReset,
        sdk::ChatEvent::UserMessagesWithdrawn { texts } => UiEvent::UserMessagesWithdrawn(texts),
        sdk::ChatEvent::GraphPhaseChanged { node, .. } => UiEvent::GraphPhaseChanged { node },
        sdk::ChatEvent::Result(result) => UiEvent::SystemMessage(result.text),
    }
}

pub(crate) struct SpawnContextRefs {
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
}

pub(crate) struct SpawnContext {
    pub tx: mpsc::Sender<UiEvent>,
    pub input_event_port: TuiInputEventPort,
    pub agent_client: Arc<dyn sdk::AgentClient>,
    pub fallback_context: UiTurnContext,
    pub messages: Vec<sdk::ChatMessage>,
}

#[derive(Debug)]
pub(crate) struct ProcessingHandle {
    join: tokio::task::JoinHandle<()>,
}

impl ProcessingHandle {
    pub(crate) fn abort(&self) {
        self.join.abort();
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.join.is_finished()
    }
}

fn log_sdk_tool_event(event: &sdk::ChatEvent, stage: &'static str) {
    match event {
        sdk::ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => crate::tui::log_trace!(
            "{} tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            name,
            index
        ),
        sdk::ChatEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => crate::tui::log_trace!(
            "{} tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={} ",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            name,
            index,
            status,
            arguments_delta.as_ref().map(|value| value.len()).unwrap_or(0),
            arguments.is_some(),
        ),
        sdk::ChatEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
            ..
        } => crate::tui::log_trace!(
            "{} tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            tool_name,
            output.len(),
            json_value_kind(content),
            is_error,
            images.len()
        ),
        _ => {}
    }
}

fn log_ui_tool_event(event: &UiEvent, stage: &'static str) {
    match event {
        UiEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => crate::tui::log_trace!(
            "{} tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            name,
            index
        ),
        UiEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => crate::tui::log_trace!(
            "{} tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={} ",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            name,
            index,
            status,
            arguments_delta.as_ref().map(|value| value.len()).unwrap_or(0),
            arguments.is_some(),
        ),
        UiEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => crate::tui::log_trace!(
            "{} tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            tool_name,
            output.len(),
            json_value_kind(content),
            is_error,
            images.len()
        ),
        _ => {}
    }
}

fn json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

pub(crate) fn spawn_processing(ctx: SpawnContext) -> ProcessingHandle {
    let join = tokio::spawn(async move {
        let mut stream = match ctx
            .agent_client
            .chat(sdk::ChatRequest {
                messages: ctx.messages,
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
        while let Some(event) = stream.recv().await {
            log_sdk_tool_event(&event, "sdk->ui.recv");
            let ui_event = sdk_event_to_ui_event(event);
            log_ui_tool_event(&ui_event, "sdk->ui.mapped");
            if ctx.tx.send(ui_event).await.is_err() {
                return;
            }
        }
    });
    ProcessingHandle { join }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use sdk::ChatInputEventPort as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::watch;

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
    fn test_sdk_event_to_ui_event_maps_messages_sync() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::MessagesSync(vec![
            sdk::ChatMessage::user_text("hello"),
        ]));

        match event {
            UiEvent::MessagesSync(messages) => assert_eq!(messages[0].text_content(), "hello"),
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
    fn test_sdk_event_to_ui_event_maps_tasks_changed() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::TasksChanged);

        assert!(matches!(event, UiEvent::TaskStatusChanged));
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
            messages: Vec::new(),
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
        fn session_snapshot(&self) -> sdk::SessionSnapshot {
            sdk::SessionSnapshot {
                id: "test-session".to_string(),
                message_count: 0,
                total_tokens: 0,
                messages: vec![],
                created_at: None,
                trimmed: 0,
                repaired: 0,
                workspace: None,
                tasks: None,
            }
        }

        fn cost(&self) -> sdk::CostInfo {
            sdk::CostInfo::default()
        }

        fn task_list(&self) -> Vec<sdk::TaskSummary> {
            Vec::new()
        }

        async fn task_status(&self) -> Result<sdk::TaskStatusView, sdk::SdkError> {
            Ok(sdk::TaskStatusView::default())
        }

        fn project(&self) -> sdk::ProjectContext {
            sdk::ProjectContext::default()
        }

        fn changes(&self) -> watch::Receiver<sdk::ChangeSet> {
            let (_tx, rx) = watch::channel(sdk::ChangeSet::empty());
            rx
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

        async fn sync_current_messages(
            &self,
            _messages: Vec<sdk::ChatMessage>,
        ) -> Result<(), sdk::SdkError> {
            self.sync_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn save_current_session(&self) -> Result<(), sdk::SdkError> {
            Ok(())
        }

        fn cancel(&self) {}

        async fn load_session(&self, _id: &str) -> Result<sdk::SessionSnapshot, sdk::SdkError> {
            Ok(self.session_snapshot())
        }

        async fn list_sessions(&self) -> Result<Vec<sdk::SessionSummary>, sdk::SdkError> {
            Ok(Vec::new())
        }

        async fn delete_session(&self, _id: &str) -> Result<(), sdk::SdkError> {
            Ok(())
        }

        async fn list_models(&self) -> Result<Vec<sdk::ModelSummary>, sdk::SdkError> {
            Ok(Vec::new())
        }

        async fn compact(&self) -> Result<(), sdk::SdkError> {
            Ok(())
        }

        async fn read_clipboard_image(&self) -> Result<sdk::ClipboardImageView, sdk::SdkError> {
            Err(sdk::SdkError::Internal("not implemented".to_string()))
        }

        async fn process_image_file(
            &self,
            _path: String,
        ) -> Result<sdk::ClipboardImageView, sdk::SdkError> {
            Err(sdk::SdkError::Internal("not implemented".to_string()))
        }

        async fn run_reflection(
            &self,
            _messages: Vec<sdk::ChatMessage>,
        ) -> Result<sdk::ReflectionOutputView, sdk::SdkError> {
            Err(sdk::SdkError::Internal("not implemented".to_string()))
        }

        async fn apply_reflection(
            &self,
            _output: sdk::ReflectionOutputView,
        ) -> Result<String, sdk::SdkError> {
            Ok("applied".to_string())
        }

        async fn execute_command(
            &self,
            _name: &str,
            _args: &str,
            _ctx: sdk::CommandContext,
        ) -> Result<sdk::CommandResult, sdk::SdkError> {
            Ok(sdk::CommandResult::Success("ok".to_string()))
        }

        async fn estimate_context(
            &self,
            _messages: &[sdk::ChatMessage],
            _system_prompt: &str,
        ) -> Result<sdk::ContextEstimate, sdk::SdkError> {
            Ok(sdk::ContextEstimate {
                estimated_tokens: 0,
                system_tokens: 0,
                context_size: 0,
                usage_percentage: 0.0,
            })
        }

        async fn switch_model(
            &self,
            _params: sdk::ModelSwitchParams,
        ) -> Result<sdk::ModelSwitchResult, sdk::SdkError> {
            Ok(sdk::ModelSwitchResult {
                display_name: "test/model".to_string(),
                context_window: 0,
                reasoning_active: None,
            })
        }

        async fn set_thinking(&self, _desired: Option<bool>) -> Result<bool, sdk::SdkError> {
            Ok(true)
        }

        async fn compact_messages(
            &self,
            messages: Vec<sdk::ChatMessage>,
            _system_prompt: &str,
            _context_size: usize,
        ) -> Result<(Vec<sdk::ChatMessage>, bool), sdk::SdkError> {
            Ok((messages, false))
        }

        async fn notify_hook(&self, _message: &str, _kind: &str) -> Result<(), sdk::SdkError> {
            Ok(())
        }

        async fn list_reminders(&self) -> Result<Vec<sdk::ReminderView>, sdk::SdkError> {
            Ok(Vec::new())
        }

        async fn add_reminder(&self, _content: &str) -> Result<String, sdk::SdkError> {
            Ok("test-id".to_string())
        }

        async fn complete_reminder(&self, _id: &str) -> Result<(), sdk::SdkError> {
            Ok(())
        }

        async fn get_thinking(&self) -> Result<bool, sdk::SdkError> {
            Ok(false)
        }

        async fn restore_tasks(&self, _snapshot: serde_json::Value) -> Result<(), sdk::SdkError> {
            Ok(())
        }

        async fn clear_tasks(&self) -> Result<(), sdk::SdkError> {
            Ok(())
        }
    }
}
