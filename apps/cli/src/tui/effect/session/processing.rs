use crate::tui::app::event::{StatusContextUpdate, UiEvent};
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
        sdk::ChatEvent::Done => UiEvent::Done,
        sdk::ChatEvent::DoneWithDurationMs(ms) => {
            UiEvent::DoneWithDuration(std::time::Duration::from_millis(ms))
        }
        sdk::ChatEvent::Cancelled => UiEvent::Cancelled,
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
            id,
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        },
        sdk::ChatEvent::AgentProgress { tool_id, event } => {
            UiEvent::AgentProgress { tool_id, event }
        }
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
    pub messages: Vec<sdk::ChatMessage>,
}

pub fn spawn_processing(ctx: SpawnContext) {
    tokio::spawn(async move {
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
                let _ = ctx.tx.send(UiEvent::Done).await;
                return;
            }
        };
        let queue = TuiQueueDrainPort::new(ctx.queue_request_tx);
        while let Some(event) = stream.recv().await {
            let ui_event = sdk_event_to_ui_event(event);
            let is_done = matches!(ui_event, UiEvent::Done | UiEvent::DoneWithDuration(_));
            if ctx.tx.send(ui_event).await.is_err() {
                return;
            }
            if is_done {
                if let Some(queued) = queue.drain_queued_input().await {
                    let messages = queued
                        .into_iter()
                        .map(|text| sdk::ChatMessage {
                            role: "user".to_string(),
                            content: serde_json::json!([{ "type": "text", "text": text }]),
                        })
                        .collect();
                    if let Err(e) = ctx.agent_client.sync_current_messages(messages).await {
                        log::warn!("failed to sync drained queue messages: {e}");
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sdk_event_context() -> sdk::ChatEventContext {
        sdk::ChatEventContext::new("chat-test", "turn-test")
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
    fn test_sdk_event_to_ui_event_maps_messages_sync() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::MessagesSync(vec![sdk::ChatMessage {
            role: "user".to_string(),
            content: serde_json::json!([{ "type": "text", "text": "hello" }]),
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
}
