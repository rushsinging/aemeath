use crate::tui::app::event::{StatusContextUpdate, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::runtime::workspace::WorktreeKind;

#[derive(Debug, Default, PartialEq)]
pub struct AgentEventMapping {
    pub conversation: Vec<ConversationIntent>,
    pub diagnostic: Vec<DiagnosticIntent>,
    pub runtime: Vec<RuntimeIntent>,
    pub session: Vec<SessionIntent>,
    pub effects: Vec<Effect>,
}

pub fn map_agent_event(event: &UiEvent) -> AgentEventMapping {
    match event {
        UiEvent::Text(text) => {
            conversation(ConversationIntent::ObserveAssistantText { text: text.clone() })
        }
        UiEvent::Thinking(text) => {
            conversation(ConversationIntent::ObserveThinkingText { text: text.clone() })
        }
        UiEvent::TextBlockComplete(_) => conversation(ConversationIntent::CompleteTextBlock),
        UiEvent::ToolCallStart { name, index } => {
            conversation(ConversationIntent::ObserveToolCallStart {
                name: name.clone(),
                index: *index,
            })
        }
        UiEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        } => conversation(ConversationIntent::ObserveToolArguments {
            name: name.clone(),
            index: *index,
            partial_args: partial_args.clone(),
        }),
        UiEvent::ToolCall {
            id,
            name,
            index,
            summary,
        } => conversation(ConversationIntent::ObserveToolCall {
            id: id.clone(),
            name: name.clone(),
            index: index.unwrap_or(0),
            summary: summary.clone(),
        }),
        UiEvent::ToolResult {
            id,
            tool_name,
            output,
            is_error,
            images,
        } => conversation(ConversationIntent::ObserveToolResult {
            id: id.clone(),
            tool_name: tool_name.clone(),
            output: output.clone(),
            is_error: *is_error,
            image_count: images.len(),
        }),
        UiEvent::Usage { input, output, .. } => runtime(RuntimeIntent::RecordUsage {
            input_tokens: u64::from(*input),
            output_tokens: u64::from(*output),
            cost_usd: 0.0,
        }),
        UiEvent::LiveTps(tps) => runtime(RuntimeIntent::RecordLiveTps { tps: *tps }),
        UiEvent::Error(message) => {
            let mut mapping = conversation(ConversationIntent::AppendError {
                text: message.clone(),
            });
            mapping.diagnostic.push(DiagnosticIntent::RecordNotice {
                severity: DiagnosticSeverity::Error,
                message: message.clone(),
            });
            mapping.effects.push(Effect::RunHook {
                name: "error".to_string(),
                message: message.clone(),
            });
            mapping
        }
        UiEvent::SystemMessage(text) | UiEvent::ReminderRecap(text) => {
            conversation(ConversationIntent::AppendSystemMessage { text: text.clone() })
        }
        UiEvent::MessagesSync(messages) => session(SessionIntent::MessagesSynced {
            message_count: messages.len(),
        }),
        UiEvent::AskUser { id, question, .. } => diagnostic(DiagnosticIntent::OpenPrompt {
            id: id.clone(),
            question: question.clone(),
        }),
        UiEvent::AgentProgress { tool_id, event } => {
            conversation(ConversationIntent::RecordAgentProgress {
                tool_id: tool_id.clone(),
                message: format!("{event:?}"),
            })
        }
        UiEvent::WorkingDirectoryChanged(update) => map_status_context(update),
        UiEvent::Done | UiEvent::DoneWithDuration(_) | UiEvent::Cancelled => {
            conversation(ConversationIntent::CompleteChat)
        }
        _ => AgentEventMapping::default(),
    }
}

fn conversation(intent: ConversationIntent) -> AgentEventMapping {
    AgentEventMapping {
        conversation: vec![intent],
        ..AgentEventMapping::default()
    }
}

fn diagnostic(intent: DiagnosticIntent) -> AgentEventMapping {
    AgentEventMapping {
        diagnostic: vec![intent],
        ..AgentEventMapping::default()
    }
}

fn runtime(intent: RuntimeIntent) -> AgentEventMapping {
    AgentEventMapping {
        runtime: vec![intent],
        ..AgentEventMapping::default()
    }
}

fn session(intent: SessionIntent) -> AgentEventMapping {
    AgentEventMapping {
        session: vec![intent],
        ..AgentEventMapping::default()
    }
}

fn map_status_context(update: &StatusContextUpdate) -> AgentEventMapping {
    runtime(RuntimeIntent::WorkspaceSnapshotReceived {
        path_base: Some(update.path_base.clone()),
        working_root: Some(update.working_root.clone()),
        branch: update.branch.clone(),
        kind: match update.kind {
            crate::tui::render::status::WorktreeKind::Main => WorktreeKind::MainCheckout,
            crate::tui::render::status::WorktreeKind::Worktree => WorktreeKind::LinkedWorktree,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_agent_event_text_to_conversation_intent() {
        let mapping = map_agent_event(&UiEvent::Text("hello".to_string()));
        assert!(matches!(
            mapping.conversation.first(),
            Some(ConversationIntent::ObserveAssistantText { text }) if text == "hello"
        ));
    }

    #[test]
    fn test_map_agent_event_usage_to_runtime_intent() {
        let mapping = map_agent_event(&UiEvent::Usage {
            input: 1,
            output: 2,
            last_input: 1,
            elapsed_secs: 1.0,
        });
        assert!(matches!(
            mapping.runtime.first(),
            Some(RuntimeIntent::RecordUsage {
                input_tokens: 1,
                output_tokens: 2,
                ..
            })
        ));
    }

    #[test]
    fn test_map_agent_event_error_records_diagnostic_and_hook() {
        let mapping = map_agent_event(&UiEvent::Error("坏了".to_string()));
        assert_eq!(mapping.conversation.len(), 1);
        assert_eq!(mapping.diagnostic.len(), 1);
        assert!(matches!(
            mapping.effects.first(),
            Some(Effect::RunHook { .. })
        ));
    }
}
