use crate::tui::core::event::{StatusContextUpdate, UiEvent};
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

pub(crate) fn sdk_event_to_ui_event(event: sdk::ChatEvent) -> UiEvent {
    match event {
        sdk::ChatEvent::Token(text) => UiEvent::Text(text),
        sdk::ChatEvent::Thinking(text) => UiEvent::Thinking(text),
        sdk::ChatEvent::TextBlockComplete(text) => UiEvent::TextBlockComplete(text),
        sdk::ChatEvent::ToolCallStart { name, index } => UiEvent::ToolCallStart { name, index },
        sdk::ChatEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        } => UiEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        },
        sdk::ChatEvent::ToolCall { id, name, summary } => UiEvent::ToolCall { id, name, summary },
        sdk::ChatEvent::ToolResult {
            id,
            tool_name,
            output,
            is_error,
            images,
        } => UiEvent::ToolResult {
            id,
            tool_name,
            output,
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
        sdk::ChatEvent::TurnChanged(turn) => {
            ::runtime::api::bootstrap::set_current_turn(turn);
            UiEvent::SystemMessage(String::new())
        }
        sdk::ChatEvent::StopFailureHook {
            system_message,
            additional_context,
        } => UiEvent::StopFailureHook {
            system_message,
            additional_context,
        },
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
        sdk::ChatEvent::HookStart { event, command } => UiEvent::HookStart { event, command },
        sdk::ChatEvent::HookEnd {
            event,
            blocked,
            error,
        } => UiEvent::HookEnd {
            event,
            blocked,
            error,
        },
        sdk::ChatEvent::WorkingDirectoryChanged {
            path_base,
            working_root,
            workspace,
        } => UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
            path_base: crate::tui::core::display_status_path(std::path::Path::new(&path_base)),
            working_root: crate::tui::core::display_status_path(std::path::Path::new(
                &working_root,
            )),
            branch: crate::tui::core::git_branch_for(std::path::Path::new(&working_root)),
            kind: crate::tui::core::worktree_kind_for(std::path::Path::new(&working_root)),
            raw_path_base: std::path::PathBuf::from(path_base),
            raw_working_root: std::path::PathBuf::from(working_root),
            workspace,
        }),
        sdk::ChatEvent::Result(result) => UiEvent::Text(result.text),
    }
}

pub(crate) struct SpawnContextRefs {
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
}

pub(crate) struct SpawnContext {
    pub tx: mpsc::Sender<UiEvent>,
    pub queue_request_tx: mpsc::Sender<UiEvent>,
    pub agent_client: Arc<dyn sdk::AgentClient>,
    pub messages: Vec<sdk::ChatMessage>,
}

pub fn spawn_processing(ctx: SpawnContext) {
    tokio::spawn(async move {
        let mut stream = match ctx
            .agent_client
            .chat(sdk::ChatRequest {
                messages: ctx.messages,
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

    #[test]
    fn test_sdk_event_to_ui_event_maps_token() {
        let event = sdk_event_to_ui_event(sdk::ChatEvent::Token("hello".to_string()));

        match event {
            UiEvent::Text(text) => assert_eq!(text, "hello"),
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
}
