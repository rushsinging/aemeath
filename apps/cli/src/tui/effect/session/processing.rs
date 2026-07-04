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
        sdk::ChatEvent::CompactProgress {
            stage,
            current,
            total,
        } => UiEvent::CompactProgress {
            stage,
            current,
            total,
        },
        sdk::ChatEvent::ModelSwitched { result } => UiEvent::ModelSwitched { result },
        sdk::ChatEvent::ThinkingChanged { enabled } => UiEvent::ThinkingChanged { enabled },
        sdk::ChatEvent::ContextEstimated {
            estimate,
            message_count,
        } => UiEvent::ContextEstimated {
            estimate,
            message_count,
        },
        sdk::ChatEvent::CommandResultText { text, is_error } => {
            UiEvent::CommandResultText { text, is_error }
        }
        sdk::ChatEvent::SessionResumed {
            messages,
            session_id,
            created_at,
        } => UiEvent::SessionResumed {
            messages,
            session_id,
            created_at,
        },
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

/// #567 S5：退出时等待 spawn task 完成（含 auto-save），超时则放弃。
pub(crate) async fn shutdown_and_save(handle: Option<ProcessingHandle>) {
    if let Some(handle) = handle {
        // 先 abort 如果已卡死——但给 loop 一点时间自然退出 + auto-save。
        // JoinHandle.await 在 tokio runtime 中等待 task 完成。
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), handle.join).await;
        if timeout.is_err() {
            crate::tui::log_warn!("auto-save timed out, forcing abort");
        }
    }
}

pub(crate) fn log_sdk_event(event: &sdk::ChatEvent, stage: &'static str) {
    match event {
        sdk::ChatEvent::Token { context, text } => crate::tui::log_trace!(
            "{} token chat_id={} turn_id={} text_len={}",
            stage,
            context.chat_id,
            context.turn_id,
            text.len()
        ),
        sdk::ChatEvent::Thinking { context, text } => crate::tui::log_trace!(
            "{} thinking chat_id={} turn_id={} text_len={}",
            stage,
            context.chat_id,
            context.turn_id,
            text.len()
        ),
        sdk::ChatEvent::BlockComplete { context, text } => crate::tui::log_trace!(
            "{} block_complete chat_id={} turn_id={} text_len={}",
            stage,
            context.chat_id,
            context.turn_id,
            text.len()
        ),
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
        sdk::ChatEvent::SystemMessage(message) => {
            crate::tui::log_trace!("{} system_message len={}", stage, message.len())
        }
        sdk::ChatEvent::Error(message) => {
            crate::tui::log_trace!("{} error len={}", stage, message.len())
        }
        sdk::ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => crate::tui::log_trace!(
            "{} usage input={} output={} last_input={} elapsed_secs={:.3}",
            stage,
            input,
            output,
            last_input,
            elapsed_secs
        ),
        sdk::ChatEvent::MessagesSync(messages) => {
            crate::tui::log_trace!("{} messages_sync count={}", stage, messages.len())
        }
        sdk::ChatEvent::UserMessagesAdded { items } => {
            crate::tui::log_trace!("{} user_messages_added count={}", stage, items.len())
        }
        sdk::ChatEvent::Done { context } => crate::tui::log_trace!(
            "{} done chat_id={} turn_id={}",
            stage,
            context.chat_id,
            context.turn_id
        ),
        sdk::ChatEvent::DoneWithDurationMs {
            context,
            duration_ms,
        } => crate::tui::log_trace!(
            "{} done_with_duration_ms chat_id={} turn_id={} duration_ms={}",
            stage,
            context.chat_id,
            context.turn_id,
            duration_ms
        ),
        sdk::ChatEvent::Cancelled { context } => crate::tui::log_trace!(
            "{} cancelled chat_id={} turn_id={}",
            stage,
            context.chat_id,
            context.turn_id
        ),
        sdk::ChatEvent::LiveTps(tps) => crate::tui::log_trace!("{} live_tps={:.2}", stage, tps),
        sdk::ChatEvent::TurnChanged(turn) => {
            crate::tui::log_trace!("{} turn_changed turn={}", stage, turn)
        }
        sdk::ChatEvent::CurrentTurnChanged(turn) => {
            crate::tui::log_trace!("{} current_turn_changed turn={}", stage, turn)
        }
        sdk::ChatEvent::HookEvent(event) => crate::tui::log_trace!(
            "{} hook_event name={} status={:?}",
            stage,
            event.hook_name,
            event.status
        ),
        sdk::ChatEvent::AskUserBatch { items, .. } => {
            crate::tui::log_trace!("{} ask_user_batch count={}", stage, items.len())
        }
        sdk::ChatEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => crate::tui::log_trace!(
            "{} agent_progress chat_id={} turn_id={} tool_id={} seq={} kind={}",
            stage,
            context.chat_id,
            context.turn_id,
            tool_id,
            event.sequence,
            event
        ),
        sdk::ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => crate::tui::log_trace!(
            "{} working_directory_changed path_base={} workspace_root={} context_stack_len={}",
            stage,
            path_base,
            workspace_root,
            workspace.context_stack.len()
        ),
        sdk::ChatEvent::TasksChanged => {
            crate::tui::log_trace!("{} tasks_changed", stage)
        }
        sdk::ChatEvent::ConfigReloaded { changed_keys } => crate::tui::log_trace!(
            "{} config_reloaded changed_keys={:?}",
            stage,
            changed_keys
        ),
        sdk::ChatEvent::GraphPhaseChanged {
            node,
            effort,
            prev,
        } => crate::tui::log_trace!(
            "{} graph_phase_changed node={} effort={} prev={}",
            stage,
            node,
            effort,
            prev
        ),
        sdk::ChatEvent::SessionReset => {
            crate::tui::log_trace!("{} session_reset", stage)
        }
        sdk::ChatEvent::UserMessagesWithdrawn { texts } => crate::tui::log_trace!(
            "{} user_messages_withdrawn count={}",
            stage,
            texts.len()
        ),
        sdk::ChatEvent::CompactProgress {
            stage: _,
            current,
            total,
        } => crate::tui::log_trace!(
            "{} compact_progress current={:?} total={:?}",
            stage,
            current,
            total,
        ),
        sdk::ChatEvent::ModelSwitched { result } => crate::tui::log_trace!(
            "{} model_switched display={} context_window={} reasoning={:?}",
            stage,
            result.display_name,
            result.context_window,
            result.reasoning_active
        ),
        sdk::ChatEvent::ThinkingChanged { enabled } => {
            crate::tui::log_trace!("{} thinking_changed enabled={}", stage, enabled)
        }
        sdk::ChatEvent::ContextEstimated {
            estimate,
            message_count,
        } => crate::tui::log_trace!(
            "{} context_estimated tokens={} system={} size={} pct={} msgs={}",
            stage,
            estimate.estimated_tokens,
            estimate.system_tokens,
            estimate.context_size,
            estimate.usage_percentage,
            message_count
        ),
        sdk::ChatEvent::CommandResultText { text, is_error } => crate::tui::log_trace!(
            "{} command_result_text len={} is_error={}",
            stage,
            text.len(),
            is_error
        ),
        sdk::ChatEvent::SessionResumed { messages, session_id, .. } => crate::tui::log_trace!(
            "{} session_resumed id={} msg_count={}",
            stage,
            session_id,
            messages.len()
        ),
        sdk::ChatEvent::Result(result) => crate::tui::log_trace!(
            "{} result text_len={} tokens_used={:?}",
            stage,
            result.text.len(),
            result.tokens_used
        ),
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
            log_sdk_event(&event, "sdk->ui.recv");
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

        async fn notify_hook(&self, _message: &str, _kind: &str) -> Result<(), sdk::SdkError> {
            Ok(())
        }

        async fn list_reminders(&self) -> Result<Vec<sdk::ReminderView>, sdk::SdkError> {
            Ok(Vec::new())
        }

        async fn restore_tasks(&self, _snapshot: serde_json::Value) -> Result<(), sdk::SdkError> {
            Ok(())
        }
    }
}
