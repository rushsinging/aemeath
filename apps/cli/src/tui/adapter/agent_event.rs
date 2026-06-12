use crate::tui::adapter::hook_notice::{hook_event_notice, hook_spinner_phase};
use crate::tui::app::event::{StatusContextUpdate, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;
use crate::tui::model::runtime::workspace::WorktreeKind;
use crate::tui::render::display::safe_text::safe_str_slice_by_char;
use serde_json::{Map, Value};

const TOOL_TEXT_PREVIEW_LIMIT: usize = 16 * 1024;
const TOOL_STREAM_PREVIEW_LIMIT: usize = 512;
const TOOL_LARGE_FIELD_PREVIEW_LIMIT: usize = 256;

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
    match event {
        UiEvent::Text { context, text } => {
            let mut mapping = conversation(ConversationIntent::BindRuntimeTurn {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            });
            mapping
                .conversation
                .push(ConversationIntent::ObserveAssistantText { text: text.clone() });
            mapping
                .runtime
                .push(RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Generating));
            mapping
        }
        UiEvent::Thinking { context, text } => {
            let mut mapping = conversation(ConversationIntent::BindRuntimeTurn {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            });
            mapping
                .conversation
                .push(ConversationIntent::ObserveThinkingText { text: text.clone() });
            mapping
                .runtime
                .push(RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Thinking));
            mapping
        }
        UiEvent::TextBlockComplete { context, .. } => {
            let mut mapping = conversation(ConversationIntent::BindRuntimeTurn {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            });
            mapping
                .conversation
                .push(ConversationIntent::CompleteTextBlock);
            mapping
        }
        UiEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => {
            let mut mapping = conversation(ConversationIntent::BindRuntimeTurn {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            });
            mapping
                .conversation
                .push(ConversationIntent::ObserveToolCallStart {
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    name: name.clone(),
                    index: *index,
                });
            mapping
        }
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
        } => {
            let mut mapping = conversation(ConversationIntent::BindRuntimeTurn {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            });
            mapping
                .conversation
                .push(ConversationIntent::ObserveToolCallUpdate {
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    name: name.clone(),
                    index: *index,
                    arguments: arguments_delta
                        .as_ref()
                        .map(|value| sanitize_tool_arguments_delta(name, value)),
                    summary: summary
                        .as_ref()
                        .map(|value| sanitize_tool_summary(name, value))
                        .or_else(|| {
                            arguments.as_ref().map(|value| {
                                sanitize_tool_arguments(name, value.clone()).to_string()
                            })
                        }),
                    status: tool_call_status_from_sdk(*status),
                });
            mapping
        }
        UiEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => {
            let mut mapping = conversation(ConversationIntent::BindRuntimeTurn {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            });
            mapping
                .conversation
                .push(ConversationIntent::ObserveToolResult {
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    tool_name: tool_name.clone(),
                    output: sanitize_tool_output(tool_name, output),
                    content: sanitize_tool_result_content(tool_name, content.clone()),
                    is_error: *is_error,
                    image_count: images.len(),
                });
            mapping
        }
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
        UiEvent::AskUser { id, question, .. } => diagnostic(DiagnosticIntent::OpenPrompt {
            id: id.clone(),
            question: question.clone(),
        }),
        UiEvent::AgentProgress { tool_id, event } => {
            conversation(ConversationIntent::RecordAgentProgress {
                tool_id: tool_id.clone(),
                message: format!("{event}"),
            })
        }
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
        UiEvent::Done | UiEvent::DoneWithDuration(_) | UiEvent::Cancelled => {
            conversation(ConversationIntent::CompleteChat)
        }
        _ => AgentEventMapping::default(),
    }
}

fn sanitize_tool_arguments_delta(tool_name: &str, partial_args: &str) -> String {
    truncate_tool_text(partial_args, TOOL_STREAM_PREVIEW_LIMIT, Some(tool_name))
}

fn sanitize_tool_arguments(tool_name: &str, arguments: Value) -> Value {
    sanitize_tool_value(tool_name, arguments)
}

fn sanitize_tool_summary(tool_name: &str, summary: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(summary) else {
        return truncate_large_tool_text(summary, Some(tool_name));
    };
    sanitize_tool_value(tool_name, value).to_string()
}

fn sanitize_tool_output(tool_name: &str, output: &str) -> String {
    truncate_large_tool_text(output, Some(tool_name))
}

fn sanitize_tool_result_content(tool_name: &str, content: Value) -> Value {
    match content {
        Value::Object(object) => sanitize_tool_value(tool_name, Value::Object(object)),
        value => truncate_json_value(value, tool_name, "content"),
    }
}

fn sanitize_tool_value(tool_name: &str, value: Value) -> Value {
    let Value::Object(mut object) = value else {
        return truncate_json_value(value, tool_name, "value");
    };
    for field in large_fields_for_tool(tool_name) {
        summarize_object_string_field(&mut object, tool_name, field);
    }
    Value::Object(object)
}

fn large_fields_for_tool(tool_name: &str) -> &'static [&'static str] {
    match tool_name {
        "Write" => &["content"],
        "Edit" => &["old_string", "new_string"],
        "Agent" => &["prompt"],
        "Bash" => &["command"],
        "AskUserQuestion" => &["question"],
        _ => &[],
    }
}

fn summarize_object_string_field(object: &mut Map<String, Value>, tool_name: &str, field: &str) {
    let Some(value) = object.get_mut(field) else {
        return;
    };
    let Some(text) = value.as_str() else {
        return;
    };
    if text.len() <= TOOL_LARGE_FIELD_PREVIEW_LIMIT {
        return;
    }
    *value = Value::String(format!(
        "{}\n... ({} bytes omitted from TUI {tool_name}.{field} preview)",
        utf8_prefix(text, TOOL_LARGE_FIELD_PREVIEW_LIMIT),
        text.len()
            .saturating_sub(utf8_prefix(text, TOOL_LARGE_FIELD_PREVIEW_LIMIT).len())
    ));
}

fn truncate_json_value(value: Value, tool_name: &str, field: &str) -> Value {
    let text = value.to_string();
    Value::String(truncate_tool_text(
        &text,
        TOOL_TEXT_PREVIEW_LIMIT,
        Some(&format!("{tool_name}.{field}")),
    ))
}

fn truncate_large_tool_text(text: &str, context: Option<&str>) -> String {
    truncate_tool_text(text, TOOL_TEXT_PREVIEW_LIMIT, context)
}

fn truncate_tool_text(text: &str, limit: usize, context: Option<&str>) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let prefix = utf8_prefix(text, limit);
    let omitted = text.len().saturating_sub(prefix.len());
    let suffix = match context {
        Some(context) => format!("... ({omitted} bytes omitted from TUI preview for {context})"),
        None => format!("... ({omitted} bytes omitted from TUI preview)"),
    };
    format!("{prefix}\n{suffix}")
}

fn utf8_prefix(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let char_end = text
        .char_indices()
        .take_while(|(idx, ch)| idx + ch.len_utf8() <= limit)
        .count();
    safe_str_slice_by_char(text, 0, char_end)
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
    use crate::tui::app::event::UiTurnContext;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};

    fn ctx() -> UiTurnContext {
        UiTurnContext {
            chat_id: ChatId::new("chat-test"),
            turn_id: ChatTurnId::new("turn-test"),
        }
    }

    fn first_observation(mapping: &AgentEventMapping) -> Option<&ConversationIntent> {
        mapping
            .conversation
            .iter()
            .find(|intent| !matches!(intent, ConversationIntent::BindRuntimeTurn { .. }))
    }

    #[test]
    fn test_map_agent_event_text_to_conversation_intent() {
        let mapping = map_agent_event(&UiEvent::Text {
            context: ctx(),
            text: "hello".to_string(),
        });
        assert!(matches!(
            first_observation(&mapping),
            Some(ConversationIntent::ObserveAssistantText { text }) if text == "hello"
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
            Some(ConversationIntent::ObserveAssistantText { text }) if text == "hello"
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
            Some(ConversationIntent::ObserveThinkingText { text }) if text == "reason"
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
    fn test_map_agent_event_tool_call_uses_json_arguments_when_summary_missing() {
        let event = UiEvent::ToolCallUpdate {
            context: ctx(),
            id: "tool-1".to_string(),
            provider_id: Some("provider-1".to_string()),
            name: "Read".to_string(),
            index: 0,
            arguments_delta: None,
            arguments: Some(serde_json::json!({ "file_path": "src/lib.rs" })),
            summary: None,
            status: sdk::ToolCallStatusView::Ready,
        };
        let mapping = map_agent_event(&event);

        match first_observation(&mapping) {
            Some(ConversationIntent::ObserveToolCallUpdate {
                arguments, summary, ..
            }) => {
                assert!(arguments.is_none());
                assert_eq!(summary.as_deref(), Some(r#"{"file_path":"src/lib.rs"}"#));
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

    #[test]
    fn test_map_agent_event_tool_update_truncates_large_stream() {
        let event = UiEvent::ToolCallUpdate {
            context: ctx(),
            id: "tool-1".to_string(),
            provider_id: Some("provider-1".to_string()),
            name: "Edit".to_string(),
            index: 0,
            arguments_delta: Some("x".repeat(TOOL_STREAM_PREVIEW_LIMIT * 2)),
            arguments: None,
            summary: None,
            status: sdk::ToolCallStatusView::PendingArgs,
        };

        let mapping = map_agent_event(&event);

        match first_observation(&mapping) {
            Some(ConversationIntent::ObserveToolCallUpdate { arguments, .. }) => {
                let arguments = arguments.as_deref().unwrap_or_default();
                assert!(arguments.len() < TOOL_STREAM_PREVIEW_LIMIT + 128);
                assert!(arguments.contains("omitted from TUI preview for Edit"));
            }
            other => panic!("unexpected mapping: {other:?}"),
        }
    }
    #[test]
    fn test_map_agent_event_tool_call_summarizes_edit_fields() {
        let large_old = "旧".repeat(TOOL_LARGE_FIELD_PREVIEW_LIMIT);
        let large_new = "新".repeat(TOOL_LARGE_FIELD_PREVIEW_LIMIT);
        let event = UiEvent::ToolCallUpdate {
            context: ctx(),
            id: "tool-1".to_string(),
            provider_id: Some("provider-1".to_string()),
            name: "Edit".to_string(),
            index: 0,
            arguments_delta: None,
            arguments: None,
            summary: Some(
                serde_json::json!({
                    "file_path": "src/lib.rs",
                    "old_string": large_old,
                    "new_string": large_new,
                })
                .to_string(),
            ),
            status: sdk::ToolCallStatusView::Ready,
        };
        let mapping = map_agent_event(&event);

        match first_observation(&mapping) {
            Some(ConversationIntent::ObserveToolCallUpdate { summary, .. }) => {
                let summary = summary.as_deref().unwrap_or_default();
                assert!(summary.contains("src/lib.rs"));
                assert!(summary.contains("Edit.old_string"));
                assert!(summary.contains("Edit.new_string"));
                assert!(
                    summary.len() < 1_200,
                    "summary too large: {}",
                    summary.len()
                );
            }
            other => panic!("unexpected mapping: {other:?}"),
        }
    }

    #[test]
    fn test_map_agent_event_tool_call_summarizes_agent_prompt_and_bash_command() {
        for (tool_name, field) in [("Agent", "prompt"), ("Bash", "command")] {
            let event = UiEvent::ToolCallUpdate {
                context: ctx(),
                id: "tool-1".to_string(),
                provider_id: Some("provider-1".to_string()),
                name: tool_name.to_string(),
                index: 0,
                arguments_delta: None,
                arguments: None,
                summary: Some(
                    serde_json::json!({ field: "x".repeat(TOOL_LARGE_FIELD_PREVIEW_LIMIT * 2) })
                        .to_string(),
                ),
                status: sdk::ToolCallStatusView::Ready,
            };
            let mapping = map_agent_event(&event);

            match first_observation(&mapping) {
                Some(ConversationIntent::ObserveToolCallUpdate { summary, .. }) => {
                    let summary = summary.as_deref().unwrap_or_default();
                    assert!(summary.contains(&format!("{tool_name}.{field}")));
                    assert!(
                        summary.len() < 700,
                        "{tool_name} summary too large: {}",
                        summary.len()
                    );
                }
                other => panic!("unexpected mapping: {other:?}"),
            }
        }
    }

    #[test]
    fn test_map_agent_event_tool_result_truncates_large_output() {
        let event = UiEvent::ToolResult {
            context: ctx(),
            id: "tool-1".to_string(),
            provider_id: "provider-1".to_string(),
            tool_name: "Bash".to_string(),
            output: "x".repeat(TOOL_TEXT_PREVIEW_LIMIT * 2),
            content: serde_json::json!({ "text": "x".repeat(TOOL_TEXT_PREVIEW_LIMIT * 2) }),
            is_error: false,
            images: Vec::new(),
        };

        let mapping = map_agent_event(&event);

        match first_observation(&mapping) {
            Some(ConversationIntent::ObserveToolResult { output, .. }) => {
                assert!(output.len() < TOOL_TEXT_PREVIEW_LIMIT + 128);
                assert!(output.contains("omitted from TUI preview for Bash"));
            }
            other => panic!("unexpected mapping: {other:?}"),
        }
    }

    #[test]
    fn test_truncate_tool_text_preserves_utf8_boundary() {
        let text = format!("{}😀", "你".repeat(10));
        let truncated = truncate_tool_text(&text, 31, None);

        assert!(truncated.contains("你"));
        assert!(truncated.contains("omitted from TUI preview"));
    }
}
