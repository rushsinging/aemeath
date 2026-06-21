use crate::business::chat::{RuntimeHookEvent, RuntimeHookEventStatus};
use crate::LOG_TARGET;
use std::sync::{Arc, Mutex};

use crate::business::chat::looping::RuntimeTurnContext;
use sdk::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChangeSet, ChatEvent,
    ChatEventContext, HookEventStatus, HookEventView, HookExecutionResultView, ToolCallStatusView,
    ToolResultImage,
};

#[derive(Clone)]
pub(crate) struct SdkChatEventSink {
    pub(super) tx: tokio::sync::mpsc::UnboundedSender<ChatEvent>,
    pub(super) current_messages: Arc<Mutex<Vec<share::message::Message>>>,
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
                &self.change_tx,
            ));
        })
    }

    fn try_send_event(&self, event: crate::business::chat::RuntimeStreamEvent) {
        let _ = self.tx.send(runtime_event_to_sdk_event(
            event,
            &self.current_messages,
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

    fn recv_next_input<'a>(&'a self) -> crate::business::chat::InputEventOptFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(port) => port.recv_next().await,
                None => None,
            }
        })
    }
}

fn turn_context_to_sdk(context: RuntimeTurnContext) -> ChatEventContext {
    ChatEventContext::new(context.chat_id, context.turn_id)
}

fn tool_call_status_to_sdk(
    status: crate::business::chat::RuntimeToolCallStatus,
) -> ToolCallStatusView {
    match status {
        crate::business::chat::RuntimeToolCallStatus::PendingArgs => {
            ToolCallStatusView::PendingArgs
        }
        crate::business::chat::RuntimeToolCallStatus::Ready => ToolCallStatusView::Ready,
        crate::business::chat::RuntimeToolCallStatus::Running => ToolCallStatusView::Running,
    }
}

pub(crate) fn runtime_event_to_sdk_event(
    event: crate::business::chat::RuntimeStreamEvent,
    current_messages: &Arc<Mutex<Vec<share::message::Message>>>,
    change_tx: &tokio::sync::watch::Sender<ChangeSet>,
) -> ChatEvent {
    match event {
        crate::business::chat::RuntimeStreamEvent::Text { context, text } => ChatEvent::Token {
            context: turn_context_to_sdk(context),
            text,
        },
        crate::business::chat::RuntimeStreamEvent::Thinking { context, text } => {
            ChatEvent::Thinking {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::business::chat::RuntimeStreamEvent::BlockComplete { context, text } => {
            ChatEvent::BlockComplete {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::business::chat::RuntimeStreamEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => {
            log::trace!(
                target: LOG_TARGET,
                "runtime->sdk tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                name,
                index
            );
            ChatEvent::ToolCallStart {
                context: turn_context_to_sdk(context),
                id,
                provider_id,
                name,
                index,
            }
        }
        crate::business::chat::RuntimeStreamEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => {
            log::trace!(
                target: LOG_TARGET,
                "runtime->sdk tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                name,
                index,
                status,
                arguments_delta.as_ref().map(|value| value.len()).unwrap_or(0),
                arguments.is_some()
            );
            ChatEvent::ToolCallUpdate {
                context: turn_context_to_sdk(context),
                id,
                provider_id,
                name,
                index,
                arguments_delta,
                arguments,
                status: tool_call_status_to_sdk(status),
            }
        }
        crate::business::chat::RuntimeStreamEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => {
            log::trace!(
                target: LOG_TARGET,
                "runtime->sdk tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                tool_name,
                output.len(),
                match &content {
                    serde_json::Value::Null => "null",
                    serde_json::Value::Bool(_) => "bool",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::String(_) => "string",
                    serde_json::Value::Array(_) => "array",
                    serde_json::Value::Object(_) => "object",
                },
                is_error,
                images.len()
            );
            ChatEvent::ToolResult {
                context: turn_context_to_sdk(context),
                id,
                provider_id,
                tool_name,
                output,
                content,
                is_error,
                images: images
                    .into_iter()
                    .map(|image| ToolResultImage {
                        base64: image.base64,
                        media_type: image.media_type,
                    })
                    .collect(),
            }
        }
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
        crate::business::chat::RuntimeStreamEvent::UserMessagesAdded { items } => {
            ChatEvent::UserMessagesAdded { items }
        }
        crate::business::chat::RuntimeStreamEvent::Done { context } => ChatEvent::Done {
            context: ChatEventContext::new(context.chat_id, context.turn_id),
        },
        crate::business::chat::RuntimeStreamEvent::DoneWithDuration { context, duration } => {
            ChatEvent::DoneWithDurationMs {
                context: ChatEventContext::new(context.chat_id, context.turn_id),
                duration_ms: duration.as_millis() as u64,
            }
        }
        crate::business::chat::RuntimeStreamEvent::Cancelled { context } => ChatEvent::Cancelled {
            context: ChatEventContext::new(context.chat_id, context.turn_id),
        },
        crate::business::chat::RuntimeStreamEvent::LiveTps(tps) => ChatEvent::LiveTps(tps),
        crate::business::chat::RuntimeStreamEvent::TurnChanged(turn) => {
            ChatEvent::CurrentTurnChanged(turn)
        }
        crate::business::chat::RuntimeStreamEvent::HookEvent(event) => {
            ChatEvent::HookEvent(runtime_hook_event_to_sdk(event))
        }
        crate::business::chat::RuntimeStreamEvent::AskUserBatch { items, reply_tx } => {
            ChatEvent::AskUserBatch { items, reply_tx }
        }
        crate::business::chat::RuntimeStreamEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => ChatEvent::AgentProgress {
            context: ChatEventContext::new(context.chat_id, context.turn_id),
            tool_id,
            event: agent_progress_event_to_sdk(event),
        },
        crate::business::chat::RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base,
            working_root,
            workspace,
        } => {
            // service 已是单一可变源，工具调用时已直接更新它；此处仅作 UI/SDK 通知，
            // 事件自带快照 DTO，无需回写任何 runtime 状态。
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
        crate::business::chat::RuntimeStreamEvent::ConfigReloaded { changed_keys } => {
            ChatEvent::ConfigReloaded { changed_keys }
        }
        crate::business::chat::RuntimeStreamEvent::SessionReset => ChatEvent::SessionReset,
        crate::business::chat::RuntimeStreamEvent::UserMessagesWithdrawn { texts } => {
            ChatEvent::UserMessagesWithdrawn { texts }
        }
    }
}

fn runtime_hook_event_to_sdk(event: RuntimeHookEvent) -> HookEventView {
    HookEventView {
        hook_name: event.hook_name,
        status: match event.status {
            RuntimeHookEventStatus::Running => HookEventStatus::Running,
            RuntimeHookEventStatus::Succeeded => HookEventStatus::Succeeded,
            RuntimeHookEventStatus::Blocked => HookEventStatus::Blocked,
            RuntimeHookEventStatus::Failed => HookEventStatus::Failed,
        },
        matcher: event.matcher,
        command: event.command,
        result: event.result.map(|result| HookExecutionResultView {
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
            decision: result.decision,
            reason: result.reason,
            additional_context: result.additional_context,
        }),
    }
}

fn agent_progress_event_to_sdk(event: share::tool::AgentProgressEvent) -> AgentProgressEventView {
    let kind = match event.kind {
        share::tool::AgentProgressKind::ToolCalls { calls } => AgentProgressKindView::ToolCalls {
            calls: calls
                .into_iter()
                .map(|call| AgentToolCallProgressView {
                    id: sdk::ids::ToolCallId::from_legacy_or_new(&call.id),
                    name: call.name,
                    input: call.input,
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
        let (change_tx, mut change_rx) = tokio::sync::watch::channel(ChangeSet::empty());

        let event = runtime_event_to_sdk_event(
            crate::business::chat::RuntimeStreamEvent::TasksChanged,
            &current_messages,
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
