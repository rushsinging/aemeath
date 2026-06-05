use std::sync::{Arc, Mutex};

use sdk::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChangeSet, ChatEvent,
    ToolResultImage,
};

#[derive(Clone)]
pub(crate) struct SdkChatEventSink {
    pub(super) tx: tokio::sync::mpsc::UnboundedSender<ChatEvent>,
    pub(super) current_messages: Arc<Mutex<Vec<share::message::Message>>>,
    pub(super) workspace_context: Arc<Mutex<Option<crate::business::session::WorkspaceContext>>>,
    pub(super) change_tx: tokio::sync::watch::Sender<ChangeSet>,
}

impl crate::business::chat::ChatEventSink for SdkChatEventSink {
    fn send_event<'a>(
        &'a self,
        event: crate::business::chat::RuntimeStreamEvent,
    ) -> crate::business::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self.tx.send(runtime_event_to_sdk_event(
                event,
                &self.current_messages,
                &self.workspace_context,
                &self.change_tx,
            ));
        })
    }

    fn try_send_event(&self, event: crate::business::chat::RuntimeStreamEvent) {
        let _ = self.tx.send(runtime_event_to_sdk_event(
            event,
            &self.current_messages,
            &self.workspace_context,
            &self.change_tx,
        ));
    }
}

#[derive(Clone, Default)]
pub(crate) struct RuntimeQueueDrainPort {
    inner: Option<Arc<dyn sdk::QueueDrainPort>>,
}

impl RuntimeQueueDrainPort {
    pub(crate) fn new(inner: Option<Arc<dyn sdk::QueueDrainPort>>) -> Self {
        Self { inner }
    }
}

impl crate::business::chat::QueueDrainPort for RuntimeQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> crate::business::chat::QueueFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(inner) => inner.drain_queued_input().await,
                None => None,
            }
        })
    }
}

#[derive(Clone, Default)]
pub(crate) struct RuntimeInputEventDrainPort {
    inner: Option<Arc<dyn sdk::ChatInputEventPort>>,
}

impl RuntimeInputEventDrainPort {
    pub(crate) fn new(inner: Option<Arc<dyn sdk::ChatInputEventPort>>) -> Self {
        Self { inner }
    }
}

impl crate::business::chat::InputEventDrainPort for RuntimeInputEventDrainPort {
    fn drain_input_events<'a>(&'a self) -> crate::business::chat::InputEventFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(inner) => inner.drain_input_events().await,
                None => Vec::new(),
            }
        })
    }
}

pub(crate) fn runtime_event_to_sdk_event(
    event: crate::business::chat::RuntimeStreamEvent,
    current_messages: &Arc<Mutex<Vec<share::message::Message>>>,
    workspace_context: &Arc<Mutex<Option<crate::business::session::WorkspaceContext>>>,
    change_tx: &tokio::sync::watch::Sender<ChangeSet>,
) -> ChatEvent {
    match event {
        crate::business::chat::RuntimeStreamEvent::Text(text) => ChatEvent::Token(text),
        crate::business::chat::RuntimeStreamEvent::Thinking(text) => ChatEvent::Thinking(text),
        crate::business::chat::RuntimeStreamEvent::TextBlockComplete(text) => {
            ChatEvent::TextBlockComplete(text)
        }
        crate::business::chat::RuntimeStreamEvent::ToolCallStart {
            id,
            provider_id,
            name,
            index,
        } => ChatEvent::ToolCallStart {
            id,
            provider_id,
            name,
            index,
        },
        crate::business::chat::RuntimeStreamEvent::ToolArgumentsDelta {
            id,
            provider_id,
            index,
            name,
            partial_args,
        } => ChatEvent::ToolArgumentsDelta {
            id,
            provider_id,
            index,
            name,
            partial_args,
        },
        crate::business::chat::RuntimeStreamEvent::ToolCall {
            id,
            provider_id,
            name,
            index,
            summary,
        } => ChatEvent::ToolCall {
            id,
            provider_id,
            name,
            index,
            summary,
        },
        crate::business::chat::RuntimeStreamEvent::ToolResult {
            id,
            provider_id,
            tool_name,
            output,
            is_error,
            images,
        } => ChatEvent::ToolResult {
            id,
            provider_id,
            tool_name,
            output,
            is_error,
            images: images
                .into_iter()
                .map(|image| ToolResultImage {
                    base64: image.base64,
                    media_type: image.media_type,
                })
                .collect(),
        },
        crate::business::chat::RuntimeStreamEvent::SystemMessage(msg) => {
            ChatEvent::SystemMessage(msg)
        }
        crate::business::chat::RuntimeStreamEvent::Error(msg) => ChatEvent::Error(msg),
        crate::business::chat::RuntimeStreamEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        crate::business::chat::RuntimeStreamEvent::MessagesSync(messages) => {
            if let Ok(mut guard) = current_messages.lock() {
                *guard = messages.clone();
            }
            ChatEvent::MessagesSync(
                messages
                    .into_iter()
                    .map(super::mapping::message_to_sdk)
                    .collect(),
            )
        }
        crate::business::chat::RuntimeStreamEvent::Done => ChatEvent::Done,
        crate::business::chat::RuntimeStreamEvent::DoneWithDuration(duration) => {
            ChatEvent::DoneWithDurationMs(duration.as_millis() as u64)
        }
        crate::business::chat::RuntimeStreamEvent::Cancelled => ChatEvent::Cancelled,
        crate::business::chat::RuntimeStreamEvent::LiveTps(tps) => ChatEvent::LiveTps(tps),
        crate::business::chat::RuntimeStreamEvent::TurnChanged(turn) => {
            ChatEvent::CurrentTurnChanged(turn)
        }
        crate::business::chat::RuntimeStreamEvent::StopFailureHook {
            system_message,
            additional_context,
        } => ChatEvent::StopFailureHook {
            system_message,
            additional_context,
        },
        crate::business::chat::RuntimeStreamEvent::AskUser {
            id,
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        } => ChatEvent::AskUser {
            id,
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        },
        crate::business::chat::RuntimeStreamEvent::AgentProgress { tool_id, event } => {
            ChatEvent::AgentProgress {
                tool_id,
                event: agent_progress_event_to_sdk(event),
            }
        }
        crate::business::chat::RuntimeStreamEvent::HookStart { event, command } => {
            ChatEvent::HookStart { event, command }
        }
        crate::business::chat::RuntimeStreamEvent::HookEnd {
            event,
            blocked,
            error,
        } => ChatEvent::HookEnd {
            event,
            blocked,
            error,
        },
        crate::business::chat::RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base,
            working_root,
            workspace,
        } => {
            if let Ok(mut guard) = workspace_context.lock() {
                *guard = Some(workspace.clone());
            }
            let previous = *change_tx.borrow();
            let _ = change_tx.send(previous | ChangeSet::PROJECT);
            ChatEvent::WorkingDirectoryChanged {
                path_base,
                working_root,
                workspace: super::mapping::workspace_context_to_sdk(workspace),
            }
        }
        crate::business::chat::RuntimeStreamEvent::TasksChanged => {
            let previous = *change_tx.borrow();
            let _ = change_tx.send(previous | ChangeSet::TASKS);
            ChatEvent::TasksChanged
        }
    }
}

fn agent_progress_event_to_sdk(event: share::tool::AgentProgressEvent) -> AgentProgressEventView {
    let kind = match event.kind {
        share::tool::AgentProgressKind::ToolCalls { calls } => AgentProgressKindView::ToolCalls {
            calls: calls
                .into_iter()
                .map(|call| AgentToolCallProgressView {
                    id: call.id,
                    name: call.name,
                    input: call.input,
                    summary: call.summary,
                })
                .collect(),
        },
        share::tool::AgentProgressKind::Message { text } => AgentProgressKindView::Message { text },
    };
    AgentProgressEventView {
        sequence: event.sequence,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
    fn test_runtime_tasks_changed_emits_sdk_event_and_change_set() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<ChatEvent>();
        let current_messages = Arc::new(Mutex::new(Vec::new()));
        let workspace_context = Arc::new(Mutex::new(None));
        let (change_tx, mut change_rx) = tokio::sync::watch::channel(ChangeSet::empty());

        let event = runtime_event_to_sdk_event(
            crate::business::chat::RuntimeStreamEvent::TasksChanged,
            &current_messages,
            &workspace_context,
            &change_tx,
        );

        assert!(matches!(event, ChatEvent::TasksChanged));
        assert!(change_rx.borrow_and_update().contains(ChangeSet::TASKS));
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
}
