use crate::tui::app::event::{StatusContextUpdate, UiEvent, UiTurnContext};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub(crate) struct TuiQueueDrainPort {
    tx: mpsc::Sender<UiEvent>,
}

impl TuiQueueDrainPort {
    pub(crate) fn new(tx: mpsc::Sender<UiEvent>) -> Self {
        Self { tx }
    }

    pub(crate) async fn drain_queued_input(&self) -> Option<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if self
            .tx
            .send(UiEvent::DrainQueuedInput { reply_tx })
            .await
            .is_err()
        {
            return None;
        }
        match reply_rx.await {
            Ok(queued) if !queued.is_empty() => Some(queued),
            _ => None,
        }
    }
}

impl sdk::QueueDrainPort for TuiQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> sdk::QueueFuture<'a> {
        Box::pin(async move { self.drain_queued_input().await })
    }
}

#[derive(Clone)]
pub(crate) struct TuiInputEventPort {
    buffer: Arc<std::sync::Mutex<Vec<sdk::ChatInputEvent>>>,
}

impl TuiInputEventPort {
    pub(crate) fn new(buffer: Arc<std::sync::Mutex<Vec<sdk::ChatInputEvent>>>) -> Self {
        Self { buffer }
    }

    pub(crate) async fn drain_input_events(&self) -> Vec<sdk::ChatInputEvent> {
        match self.buffer.lock() {
            Ok(mut events) => events.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }
}

impl sdk::ChatInputEventPort for TuiInputEventPort {
    fn drain_input_events<'a>(&'a self) -> sdk::InputEventFuture<'a> {
        Box::pin(async move { self.drain_input_events().await })
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
            summary,
            status,
        } => UiEvent::ToolCallUpdate {
            context: context.into(),
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            summary,
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
        sdk::ChatEvent::AskUser {
            id,
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        } => UiEvent::AskUser {
            id: sdk::ids::ToolCallId::from_legacy_or_new(&id),
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        },
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
            working_root,
            workspace,
        } => UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
            path_base: crate::tui::app::display_status_path(std::path::Path::new(&path_base)),
            working_root: crate::tui::app::display_status_path(std::path::Path::new(&working_root)),
            branch: crate::tui::app::git_branch_for(std::path::Path::new(&working_root)),
            kind: crate::tui::app::worktree_kind_for(std::path::Path::new(&working_root)),
            raw_path_base: std::path::PathBuf::from(path_base),
            raw_working_root: std::path::PathBuf::from(working_root),
            workspace,
        }),
        sdk::ChatEvent::TasksChanged => UiEvent::TaskStatusChanged,
        sdk::ChatEvent::Result(result) => UiEvent::SystemMessage(result.text),
    }
}

pub(crate) struct SpawnContextRefs {
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
}

pub(crate) struct SpawnContext {
    pub tx: mpsc::Sender<UiEvent>,
    pub queue_request_tx: mpsc::Sender<UiEvent>,
    pub input_event_buffer: Arc<std::sync::Mutex<Vec<sdk::ChatInputEvent>>>,
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
        } => log::trace!(
            target: "cli::tui::tool_flow",
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
            summary,
            status,
        } => log::trace!(
            target: "cli::tui::tool_flow",
            "{} tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={} summary_len={}",
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
            summary.as_ref().map(|value| value.len()).unwrap_or(0)
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
        } => log::trace!(
            target: "cli::tui::tool_flow",
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
        } => log::trace!(
            target: "cli::tui::tool_flow",
            "{} tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
            stage,
            context.chat_id.as_ref(),
            context.turn_id.as_ref(),
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
            summary,
            status,
        } => log::trace!(
            target: "cli::tui::tool_flow",
            "{} tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={} summary_len={}",
            stage,
            context.chat_id.as_ref(),
            context.turn_id.as_ref(),
            id,
            provider_id,
            name,
            index,
            status,
            arguments_delta.as_ref().map(|value| value.len()).unwrap_or(0),
            arguments.is_some(),
            summary.as_ref().map(|value| value.len()).unwrap_or(0)
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
        } => log::trace!(
            target: "cli::tui::tool_flow",
            "{} tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
            stage,
            context.chat_id.as_ref(),
            context.turn_id.as_ref(),
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
                queue_drain: Some(Arc::new(TuiQueueDrainPort::new(
                    ctx.queue_request_tx.clone(),
                ))),
                input_events: Some(Arc::new(TuiInputEventPort::new(
                    ctx.input_event_buffer.clone(),
                ))),
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::watch;

    fn test_sdk_event_context() -> sdk::ChatEventContext {
        sdk::ChatEventContext::new(sdk::ids::ChatId::new("chat-test"), sdk::ids::ChatTurnId::new("turn-test"))
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
        let expected_tool_id = sdk::ids::ToolCallId::new("tool-1".to_string());
        let event = sdk_event_to_ui_event(sdk::ChatEvent::AgentProgress {
            context: sdk::ChatEventContext::new(sdk::ids::ChatId::new("chat-progress"), sdk::ids::ChatTurnId::new("turn-progress")),
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
                assert_eq!(context.chat_id, crate::tui::model::conversation::ids::ChatId::new("chat-progress"));
                assert_eq!(context.turn_id, crate::tui::model::conversation::ids::ChatTurnId::new("turn-progress"));
                assert_eq!(tool_id, expected_tool_id);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn test_sdk_event_to_ui_event_maps_messages_sync() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::MessagesSync(vec![sdk::ChatMessage {
            role: "user".to_string(),
            content: serde_json::json!([{ "type": "text", "text": "hello" }]),
            metadata: None,
        }]));

        match event {
            UiEvent::MessagesSync(messages) => assert_eq!(messages[0].text_content(), "hello"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn test_sdk_event_to_ui_event_maps_working_directory_changed() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::WorkingDirectoryChanged {
            path_base: "/tmp".to_string(),
            working_root: "/tmp".to_string(),
            workspace: sdk::WorkspaceContextView {
                path_base: "/tmp".into(),
                working_root: "/tmp".into(),
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

    /// 回归 #104：DrainQueuedInput 事件从 TUI input queue 取出排队消息后，
    /// 回显为 UserMessage 并返回给调用方，确保 TUI 显示排队消息。
    #[tokio::test]
    async fn test_drain_queued_input_returns_messages_and_clears_queue() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let port = TuiQueueDrainPort::new(tx);

        // 模拟 TUI 主线程收到 DrainQueuedInput 后回复排队消息
        let replier = tokio::spawn(async move {
            if let Some(UiEvent::DrainQueuedInput { reply_tx }) = rx.recv().await {
                let _ = reply_tx.send(vec!["排队消息A".to_string(), "排队消息B".to_string()]);
            }
        });

        let result = port.drain_queued_input().await;
        replier.await.unwrap();

        assert_eq!(
            result,
            Some(vec!["排队消息A".to_string(), "排队消息B".to_string()]),
            "drain 应返回排队的消息列表"
        );
    }

    /// 回归 #104：DrainQueuedInput 在队列为空时返回 None。
    #[tokio::test]
    async fn test_drain_queued_input_returns_none_when_empty() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let port = TuiQueueDrainPort::new(tx);

        let replier = tokio::spawn(async move {
            if let Some(UiEvent::DrainQueuedInput { reply_tx }) = rx.recv().await {
                let _ = reply_tx.send(Vec::<String>::new());
            }
        });

        let result = port.drain_queued_input().await;
        replier.await.unwrap();

        assert!(result.is_none(), "空队列应返回 None");
    }

    /// 回归 #104：DrainQueuedInput 在通道断开时返回 None。
    #[tokio::test]
    async fn test_drain_queued_input_returns_none_when_channel_dropped() {
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        drop(rx);
        let port = TuiQueueDrainPort::new(tx);

        let result = port.drain_queued_input().await;
        assert!(result.is_none(), "通道断开应返回 None");
    }

    #[tokio::test]
    async fn test_spawn_processing_done_does_not_drain_queue_again() {
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::channel(16);
        let (queue_tx, mut queue_rx) = tokio::sync::mpsc::channel(16);
        let client = Arc::new(DoneOnlyAgentClient::default());

        spawn_processing(SpawnContext {
            tx: ui_tx,
            queue_request_tx: queue_tx,
            input_event_buffer: Arc::new(std::sync::Mutex::new(Vec::new())),
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

        let drain_request =
            tokio::time::timeout(std::time::Duration::from_millis(50), queue_rx.recv()).await;
        assert!(
            !matches!(drain_request, Ok(Some(UiEvent::DrainQueuedInput { .. }))),
            "Done 后不应再次 drain TUI input queue"
        );
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
