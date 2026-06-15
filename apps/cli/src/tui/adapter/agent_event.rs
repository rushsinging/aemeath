use crate::tui::adapter::hook_notice::{hook_event_notice, hook_spinner_phase};
use crate::tui::adapter::tool_flow_projector::ToolFlowProjector;
use crate::tui::app::event::{StatusContextUpdate, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::runtime_observation::{RuntimeObservation, RuntimeTurnContext};

#[derive(Debug, Default, PartialEq)]
pub struct AgentEventMapping {
    pub conversation: Vec<ConversationIntent>,
    pub diagnostic: Vec<DiagnosticIntent>,
    pub runtime: Vec<RuntimeIntent>,
    pub session: Vec<SessionIntent>,
    pub effects: Vec<Effect>,
}

fn tool_call_status_from_sdk(status: sdk::ToolCallStatusView) -> ToolCallStatus {
    match status {
        sdk::ToolCallStatusView::PendingArgs => ToolCallStatus::PendingArgs,
        sdk::ToolCallStatusView::Ready => ToolCallStatus::Ready,
        sdk::ToolCallStatusView::Running => ToolCallStatus::Running,
    }
}

pub fn map_agent_event(event: &UiEvent) -> AgentEventMapping {
    if let Some(observation) = runtime_observation_from_ui_event(event) {
        return map_runtime_observation(&observation);
    }
    match event {
        UiEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => {
            let mut mapping = runtime(RuntimeIntent::RecordUsage {
                input_tokens: u64::from(*input),
                output_tokens: u64::from(*output),
                last_input_tokens: u64::from(*last_input),
                cost_usd: 0.0,
            });
            if *elapsed_secs > 0.0 {
                mapping.runtime.push(RuntimeIntent::RecordLiveTps {
                    tps: f64::from(*output) / elapsed_secs,
                });
            }
            mapping
        }
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
        UiEvent::AskUser { .. } => AgentEventMapping::default(),
        UiEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => conversation(ConversationIntent::RecordAgentProgress {
            chat_id: context.chat_id.clone(),
            turn_id: context.turn_id.clone(),
            tool_id: tool_id.clone(),
            message: format!("{event}"),
        }),
        UiEvent::HookEvent(event) => {
            let mut mapping = runtime(RuntimeIntent::SetSpinnerPhase(hook_spinner_phase(event)));
            if let Some(notice) = hook_event_notice(event) {
                mapping
                    .conversation
                    .push(ConversationIntent::AppendHookNotice { content: notice });
            }
            mapping
        }
        UiEvent::WorkingDirectoryChanged(update) => map_status_context(update),
        _ => AgentEventMapping::default(),
    }
}

fn runtime_context(context: &crate::tui::app::UiTurnContext) -> RuntimeTurnContext {
    RuntimeTurnContext::new(context.chat_id.clone(), context.turn_id.clone())
}

fn runtime_observation_from_ui_event(event: &UiEvent) -> Option<RuntimeObservation> {
    match event {
        UiEvent::Text { context, text } => Some(RuntimeObservation::AssistantText {
            context: runtime_context(context),
            text: text.clone(),
        }),
        UiEvent::Thinking { context, text } => Some(RuntimeObservation::ThinkingText {
            context: runtime_context(context),
            text: text.clone(),
        }),
        UiEvent::BlockComplete { context, .. } => Some(RuntimeObservation::BlockComplete {
            context: runtime_context(context),
        }),
        UiEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => Some(RuntimeObservation::ToolCallStart {
            context: runtime_context(context),
            id: id.clone(),
            provider_id: provider_id.clone(),
            name: name.clone(),
            index: *index,
        }),
        UiEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => Some(RuntimeObservation::ToolCallUpdate {
            context: runtime_context(context),
            id: id.clone(),
            provider_id: provider_id.clone(),
            name: name.clone(),
            index: *index,
            arguments: arguments_delta.clone()
                .or_else(|| arguments.as_ref().map(ToString::to_string)),
            status: tool_call_status_from_sdk(*status),
        }),
        UiEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => Some(RuntimeObservation::ToolResult {
            context: runtime_context(context),
            id: id.clone(),
            provider_id: provider_id.clone(),
            tool_name: tool_name.clone(),
            output: output.clone(),
            content: content.clone(),
            is_error: *is_error,
            image_count: images.len(),
        }),
        UiEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => Some(RuntimeObservation::AgentProgress {
            context: runtime_context(context),
            tool_id: tool_id.clone(),
            message: format!("{event}"),
        }),
        UiEvent::Done { context }
        | UiEvent::DoneWithDuration { context, .. }
        | UiEvent::Cancelled { context } => Some(RuntimeObservation::Complete {
            context: runtime_context(context),
        }),
        _ => None,
    }
}

fn map_runtime_observation(observation: &RuntimeObservation) -> AgentEventMapping {
    ToolFlowProjector::project(observation)
}

fn conversation(intent: ConversationIntent) -> AgentEventMapping {
    AgentEventMapping {
        conversation: vec![intent],
        ..AgentEventMapping::default()
    }
}

fn _diagnostic(intent: DiagnosticIntent) -> AgentEventMapping {
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
        kind: update.kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::event::UiTurnContext;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
    use crate::tui::model::runtime::spinner::SpinnerPhase;

    fn ctx() -> UiTurnContext {
        UiTurnContext {
            chat_id: ChatId::new("chat-test"),
            turn_id: ChatTurnId::new("turn-test"),
        }
    }

    fn first_observation(mapping: &AgentEventMapping) -> Option<&ConversationIntent> {
        mapping.conversation.first()
    }

    fn assert_no_runtime_bind_prelude(mapping: &AgentEventMapping) {
        assert_eq!(
            mapping.conversation.len(),
            1,
            "runtime observations must carry context inline and emit exactly one conversation intent"
        );
    }

    #[test]
    fn test_map_agent_event_runtime_observations_do_not_emit_bind_runtime_turn() {
        let context = ctx();

        let events = vec![
            UiEvent::Text {
                context: context.clone(),
                text: "hello".to_string(),
            },
            UiEvent::Thinking {
                context: context.clone(),
                text: "thinking".to_string(),
            },
            UiEvent::BlockComplete {
                context: context.clone(),
                text: String::new(),
            },
            UiEvent::ToolCallStart {
                context: context.clone(),
                id: sdk::ids::ToolCallId::new("tool-1"),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
            },
            UiEvent::ToolCallUpdate {
                context: context.clone(),
                id: sdk::ids::ToolCallId::new("tool-1"),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
                arguments_delta: Some(r#"{"file_path":"Cargo.toml"}"#.to_string()),
                arguments: None,
                                status: sdk::ToolCallStatusView::Ready,
            },
            UiEvent::ToolResult {
                context,
                id: sdk::ids::ToolCallId::new("tool-1"),
                provider_id: "provider-1".to_string(),
                tool_name: "Read".to_string(),
                output: "ok".to_string(),
                content: serde_json::json!({ "text": "ok" }),
                is_error: false,
                images: Vec::new(),
            },
        ];

        for event in events {
            let mapping = map_agent_event(&event);
            assert_no_runtime_bind_prelude(&mapping);
        }
    }

    #[test]
    fn test_map_agent_event_text_to_conversation_intent() {
        let mapping = map_agent_event(&UiEvent::Text {
            context: ctx(),
            text: "hello".to_string(),
        });
        assert!(matches!(
            first_observation(&mapping),
            Some(ConversationIntent::ObserveAssistantText { text, .. }) if text == "hello"
        ));
    }

    #[test]
    fn test_map_agent_event_text_sets_generating_phase_with_text_update() {
        let mapping = map_agent_event(&UiEvent::Text {
            context: ctx(),
            text: "hello".to_string(),
        });

        assert!(matches!(
            first_observation(&mapping),
            Some(ConversationIntent::ObserveAssistantText { text, .. }) if text == "hello"
        ));
        assert_eq!(
            mapping.runtime,
            vec![RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Generating)]
        );
    }

    #[test]
    fn test_map_agent_event_thinking_sets_thinking_phase_with_text_update() {
        let mapping = map_agent_event(&UiEvent::Thinking {
            context: ctx(),
            text: "reason".to_string(),
        });

        assert!(matches!(
            first_observation(&mapping),
            Some(ConversationIntent::ObserveThinkingText { text, .. }) if text == "reason"
        ));
        assert_eq!(
            mapping.runtime,
            vec![RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Thinking)]
        );
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
                last_input_tokens: 1,
                ..
            })
        ));
    }

    #[test]
    fn test_map_agent_event_tool_call_fallback_uses_full_arguments_when_delta_absent() {
        let event = UiEvent::ToolCallUpdate {
            context: ctx(),
            id: sdk::ids::ToolCallId::new("tool-1"),
            provider_id: Some("provider-1".to_string()),
            name: "Read".to_string(),
            index: 0,
            arguments_delta: None,
            arguments: Some(serde_json::json!({ "file_path": "src/lib.rs" })),
            status: sdk::ToolCallStatusView::Ready,
        };
        let mapping = map_agent_event(&event);

        match first_observation(&mapping) {
            Some(ConversationIntent::ObserveToolCallUpdate {
                arguments, ..
            }) => {
                // arguments_delta 为 None 时，fallback 到 arguments JSON 字符串
                assert!(arguments.is_some());
            }
            other => panic!("unexpected mapping: {other:?}"),
        }
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
