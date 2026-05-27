use std::sync::{Arc, Mutex};

use sdk::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChatEvent,
    ToolResultImage,
};

#[derive(Clone)]
pub(crate) struct SdkChatEventSink {
    pub(super) tx: tokio::sync::mpsc::UnboundedSender<ChatEvent>,
    pub(super) current_messages: Arc<Mutex<Vec<crate::api::core::message::Message>>>,
    pub(super) workspace_context: Arc<Mutex<Option<crate::session::WorkspaceContext>>>,
}

impl crate::chat::ChatEventSink for SdkChatEventSink {
    fn send_event<'a>(
        &'a self,
        event: crate::chat::RuntimeStreamEvent,
    ) -> crate::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self.tx.send(runtime_event_to_sdk_event(
                event,
                &self.current_messages,
                &self.workspace_context,
            ));
        })
    }

    fn try_send_event(&self, event: crate::chat::RuntimeStreamEvent) {
        let _ = self.tx.send(runtime_event_to_sdk_event(
            event,
            &self.current_messages,
            &self.workspace_context,
        ));
    }
}

#[derive(Clone, Default)]
pub(crate) struct EmptyQueueDrainPort;

impl crate::chat::QueueDrainPort for EmptyQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> crate::chat::QueueFuture<'a> {
        Box::pin(async { None })
    }
}

pub(crate) fn runtime_event_to_sdk_event(
    event: crate::chat::RuntimeStreamEvent,
    current_messages: &Arc<Mutex<Vec<crate::api::core::message::Message>>>,
    workspace_context: &Arc<Mutex<Option<crate::session::WorkspaceContext>>>,
) -> ChatEvent {
    match event {
        crate::chat::RuntimeStreamEvent::Text(text) => ChatEvent::Token(text),
        crate::chat::RuntimeStreamEvent::Thinking(text) => ChatEvent::Thinking(text),
        crate::chat::RuntimeStreamEvent::TextBlockComplete(text) => {
            ChatEvent::TextBlockComplete(text)
        }
        crate::chat::RuntimeStreamEvent::ToolCallStart { name, index } => {
            ChatEvent::ToolCallStart { name, index }
        }
        crate::chat::RuntimeStreamEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        } => ChatEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        },
        crate::chat::RuntimeStreamEvent::ToolCall { id, name, summary } => {
            ChatEvent::ToolCall { id, name, summary }
        }
        crate::chat::RuntimeStreamEvent::ToolResult {
            id,
            tool_name,
            output,
            is_error,
            images,
        } => ChatEvent::ToolResult {
            id,
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
        crate::chat::RuntimeStreamEvent::SystemMessage(msg) => ChatEvent::SystemMessage(msg),
        crate::chat::RuntimeStreamEvent::Error(msg) => ChatEvent::Error(msg),
        crate::chat::RuntimeStreamEvent::Usage {
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
        crate::chat::RuntimeStreamEvent::MessagesSync(messages) => {
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
        crate::chat::RuntimeStreamEvent::Done => ChatEvent::Done,
        crate::chat::RuntimeStreamEvent::DoneWithDuration(duration) => {
            ChatEvent::DoneWithDurationMs(duration.as_millis() as u64)
        }
        crate::chat::RuntimeStreamEvent::Cancelled => ChatEvent::Cancelled,
        crate::chat::RuntimeStreamEvent::LiveTps(tps) => ChatEvent::LiveTps(tps),
        crate::chat::RuntimeStreamEvent::TurnChanged(turn) => ChatEvent::CurrentTurnChanged(turn),
        crate::chat::RuntimeStreamEvent::StopFailureHook {
            system_message,
            additional_context,
        } => ChatEvent::StopFailureHook {
            system_message,
            additional_context,
        },
        crate::chat::RuntimeStreamEvent::AskUser {
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
        crate::chat::RuntimeStreamEvent::AgentProgress { tool_id, event } => {
            ChatEvent::AgentProgress {
                tool_id,
                event: agent_progress_event_to_sdk(event),
            }
        }
        crate::chat::RuntimeStreamEvent::HookStart { event, command } => {
            ChatEvent::HookStart { event, command }
        }
        crate::chat::RuntimeStreamEvent::HookEnd {
            event,
            blocked,
            error,
        } => ChatEvent::HookEnd {
            event,
            blocked,
            error,
        },
        crate::chat::RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base,
            working_root,
            workspace,
        } => {
            if let Ok(mut guard) = workspace_context.lock() {
                *guard = Some(workspace.clone());
            }
            ChatEvent::WorkingDirectoryChanged {
                path_base,
                working_root,
                workspace: super::mapping::workspace_context_to_sdk(workspace),
            }
        }
    }
}

fn agent_progress_event_to_sdk(
    event: crate::api::core::tool::AgentProgressEvent,
) -> AgentProgressEventView {
    let kind = match event.kind {
        crate::api::core::tool::AgentProgressKind::ToolCalls { calls } => {
            AgentProgressKindView::ToolCalls {
                calls: calls
                    .into_iter()
                    .map(|call| AgentToolCallProgressView {
                        id: call.id,
                        name: call.name,
                        input: call.input,
                        summary: call.summary,
                    })
                    .collect(),
            }
        }
        crate::api::core::tool::AgentProgressKind::Message { text } => {
            AgentProgressKindView::Message { text }
        }
    };
    AgentProgressEventView {
        sequence: event.sequence,
        kind,
    }
}
