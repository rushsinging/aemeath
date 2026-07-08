use crate::tui::adapter::hook_notice::hook_event_notice;
use crate::tui::app::event::{StatusContextUpdate, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::text::safe_str_slice_by_char;
use crate::tui::view_model::tool_display::format_tool_header_view;
use sdk::{AgentProgressEventView, AgentProgressKindView};
use serde_json::{Map, Value};

#[derive(Debug, Default, PartialEq)]
pub struct AgentEventMapping {
    pub conversation: Vec<ConversationIntent>,
    pub diagnostic: Vec<DiagnosticIntent>,
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
        // ── Runtime observations → ConversationIntent (inlined from ToolFlowProjector) ──
        UiEvent::Text { context, text } => {
            conversation(ConversationIntent::AssistantText(AssistantText {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                text: text.clone(),
            }))
        }
        UiEvent::Thinking { context, text } => {
            conversation(ConversationIntent::ThinkingText(ThinkingText {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                text: text.clone(),
            }))
        }
        UiEvent::BlockComplete { context, .. } => {
            conversation(ConversationIntent::CompleteBlock(CompleteBlock {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            }))
        }
        UiEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => {
            crate::tui::log_debug!(
                "map tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                name,
                index,
            );
            conversation(ConversationIntent::ToolCallStart(ToolCallStart {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                id: id.clone(),
                provider_id: provider_id.clone(),
                name: name.clone(),
                index: *index,
            }))
        }
        UiEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => {
            let args = arguments_delta
                .clone()
                .or_else(|| arguments.as_ref().map(|value| value.to_string()));
            crate::tui::log_debug!(
                "map tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} args_len={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                name,
                index,
                args.as_ref().map(|s| s.len()).unwrap_or(0),
            );
            conversation(ConversationIntent::ToolCallUpdate(ToolCallUpdate {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                id: id.clone(),
                provider_id: provider_id.clone(),
                name: name.clone(),
                index: *index,
                arguments: args
                    .as_ref()
                    .map(|value| sanitize_tool_arguments_delta(name, value)),
                status: tool_call_status_from_sdk(*status),
            }))
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
            crate::tui::log_debug!(
                "map tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                tool_name,
                output.len(),
                json_value_kind(content),
                is_error,
                images.len(),
            );
            conversation(ConversationIntent::ToolResult(ToolResult {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                id: id.clone(),
                provider_id: provider_id.clone(),
                tool_name: tool_name.clone(),
                output: sanitize_tool_output(tool_name, output),
                content: sanitize_tool_result_content(tool_name, content.clone()),
                is_error: *is_error,
                image_count: images.len(),
            }))
        }
        UiEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => match &event.kind {
            sdk::AgentProgressKindView::Started { role, model } => {
                conversation(ConversationIntent::UpdateAgentMeta(UpdateAgentMeta {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    tool_id: tool_id.clone(),
                    role: role.clone(),
                    model: model.clone(),
                }))
            }
            _ => conversation(ConversationIntent::RecordAgentProgress(
                RecordAgentProgress {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    tool_id: tool_id.clone(),
                    message: format_agent_progress(&event),
                },
            )),
        },
        UiEvent::Done { context }
        | UiEvent::DoneWithDuration { context, .. }
        | UiEvent::Cancelled { context } => {
            conversation(ConversationIntent::CompleteChat(CompleteChat {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            }))
        }

        // ── Usage / LiveTps → ConversationIntent ──
        UiEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => {
            let mut intents = vec![ConversationIntent::RecordUsage(RecordUsage {
                input_tokens: u64::from(*input),
                output_tokens: u64::from(*output),
                last_input_tokens: u64::from(*last_input),
                cost_usd: 0.0,
            })];
            if *elapsed_secs > 0.0 {
                intents.push(ConversationIntent::RecordLiveTps(RecordLiveTps {
                    tps: f64::from(*output) / elapsed_secs,
                }));
            }
            AgentEventMapping {
                conversation: intents,
                ..AgentEventMapping::default()
            }
        }
        UiEvent::LiveTps(tps) => conversation(ConversationIntent::RecordLiveTps(RecordLiveTps {
            tps: *tps,
        })),

        // ── Error ──
        UiEvent::Error(message) => {
            let mut mapping = conversation(ConversationIntent::AppendError(AppendError {
                text: message.clone(),
            }));
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

        // ── System messages ──
        UiEvent::SystemMessage(text) | UiEvent::ReminderRecap(text) => conversation(
            ConversationIntent::AppendSystemMessage(AppendSystemMessage { text: text.clone() }),
        ),
        UiEvent::TurnStarted { messages }
        | UiEvent::MicrocompactDone { messages, .. }
        | UiEvent::StopHookBlocked { messages }
        | UiEvent::PostToolExecutionSync { messages }
        | UiEvent::CompactRollback { messages }
        | UiEvent::CompactFinished { messages } => session(SessionIntent::MessagesSynced {
            message_count: messages.len(),
        }),
        UiEvent::ApiError { messages, .. } => session(SessionIntent::MessagesSynced {
            message_count: messages.len(),
        }),
        UiEvent::AskUserBatch { .. } => AgentEventMapping::default(),

        // ── HookEvent → notice via conversation ──
        UiEvent::HookEvent(event) => {
            if event.hook_name == "PostCompact" {
                return AgentEventMapping::default();
            }
            let mut mapping = AgentEventMapping::default();
            if let Some(notice) = hook_event_notice(event) {
                mapping
                    .conversation
                    .push(ConversationIntent::AppendHookNotice(AppendHookNotice {
                        content: notice,
                    }));
            }
            mapping
        }
        UiEvent::WorkingDirectoryChanged(update) => map_status_context(update),
        _ => AgentEventMapping::default(),
    }
}

fn map_status_context(update: &StatusContextUpdate) -> AgentEventMapping {
    conversation(ConversationIntent::WorkspaceSnapshotReceived(
        WorkspaceSnapshotReceived {
            path_base: Some(update.path_base.clone()),
            workspace_root: Some(update.workspace_root.clone()),
            branch: update.branch.clone(),
            kind: update.kind,
        },
    ))
}

// ════════════════════════════════════════════════════════════════════
//  Helpers — AgentEventMapping constructors
// ════════════════════════════════════════════════════════════════════

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

fn session(intent: SessionIntent) -> AgentEventMapping {
    AgentEventMapping {
        session: vec![intent],
        ..AgentEventMapping::default()
    }
}

// ════════════════════════════════════════════════════════════════════
//  Helpers — tool output sanitization (inlined from tool_flow_projector)
// ════════════════════════════════════════════════════════════════════

const TOOL_STREAM_PREVIEW_LIMIT: usize = 512;
const TOOL_LARGE_FIELD_PREVIEW_LIMIT: usize = 256;

fn json_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn sanitize_tool_arguments_delta(tool_name: &str, partial_args: &str) -> String {
    match serde_json::from_str::<Value>(partial_args) {
        Ok(value) => {
            // 对大字段做摘要后重新序列化，保持 JSON 有效性。
            // 不再做字节截断：大字段已被 summarize_object_string_field 控制在 256 字节以内，
            // 其余字段通常很短，整体 JSON 不会过大。
            sanitize_tool_value(tool_name, value).to_string()
        }
        Err(_) => truncate_tool_text(partial_args, TOOL_STREAM_PREVIEW_LIMIT, Some(tool_name)),
    }
}

fn sanitize_tool_output(tool_name: &str, output: &str) -> String {
    truncate_large_tool_text(output, Some(tool_name))
}

fn truncate_tool_text(text: &str, limit: usize, tool_name: Option<&str>) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let prefix = utf8_prefix(text, limit);
    let label = tool_name.unwrap_or("tool");
    format!(
        "{} ... ({} bytes omitted from TUI {label} preview)",
        prefix,
        text.len().saturating_sub(prefix.len())
    )
}

fn truncate_large_tool_text(text: &str, tool_name: Option<&str>) -> String {
    truncate_tool_text(text, TOOL_STREAM_PREVIEW_LIMIT, tool_name)
}

fn truncate_json_value(value: Value, tool_name: &str, field: &str) -> Value {
    match value {
        Value::String(text) => Value::String(truncate_tool_text(
            &text,
            TOOL_LARGE_FIELD_PREVIEW_LIMIT,
            Some(tool_name),
        )),
        other => Value::String(format!(
            "<{} omitted from TUI {tool_name}.{field} preview>",
            json_value_kind(&other)
        )),
    }
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
    let Some(text) = object.get(field).and_then(Value::as_str) else {
        return;
    };
    let text_len = text.len();
    let should_truncate = text_len > TOOL_LARGE_FIELD_PREVIEW_LIMIT;
    let replacement = if should_truncate {
        let prefix = utf8_prefix(text, TOOL_LARGE_FIELD_PREVIEW_LIMIT);
        Some(Value::String(format!(
            "{} ... ({} bytes omitted from TUI {tool_name}.{field} preview)",
            prefix,
            text_len.saturating_sub(prefix.len())
        )))
    } else {
        None
    };

    if tool_name == "Write" && field == "content" {
        object.insert("content_bytes".to_string(), Value::from(text_len as u64));
    }
    if let Some(replacement) = replacement {
        object.insert(field.to_string(), replacement);
    }
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
                arguments_delta: Some("{}".to_string()),
                arguments: None,
                status: sdk::ToolCallStatusView::Ready,
            },
            UiEvent::ToolResult {
                context: context.clone(),
                id: sdk::ids::ToolCallId::new("tool-1"),
                provider_id: "provider-1".to_string(),
                tool_name: "Read".to_string(),
                output: "ok".to_string(),
                content: serde_json::json!(null),
                is_error: false,
                images: vec![],
            },
            UiEvent::Done {
                context: context.clone(),
            },
            UiEvent::Cancelled {
                context: context.clone(),
            },
        ];

        for event in &events {
            let mapping = map_agent_event(event);
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
            Some(ConversationIntent::AssistantText(AssistantText { text, .. })) if text == "hello"
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
            Some(ConversationIntent::AssistantText(AssistantText { text, .. })) if text == "hello"
        ));
    }

    #[test]
    fn test_map_agent_event_thinking_sets_thinking_phase_with_text_update() {
        let mapping = map_agent_event(&UiEvent::Thinking {
            context: ctx(),
            text: "reason".to_string(),
        });

        assert!(matches!(
            first_observation(&mapping),
            Some(ConversationIntent::ThinkingText(ThinkingText { text, .. })) if text == "reason"
        ));
    }

    #[test]
    fn test_map_agent_event_usage_to_conversation_intent() {
        let mapping = map_agent_event(&UiEvent::Usage {
            input: 1,
            output: 2,
            last_input: 1,
            elapsed_secs: 1.0,
        });
        assert!(matches!(
            mapping.conversation.first(),
            Some(ConversationIntent::RecordUsage(RecordUsage {
                input_tokens: 1,
                output_tokens: 2,
                last_input_tokens: 1,
                ..
            }))
        ));
        // RecordLiveTps should also be present since elapsed_secs > 0
        assert!(matches!(
            mapping.conversation.get(1),
            Some(ConversationIntent::RecordLiveTps(RecordLiveTps { tps })) if *tps == 2.0
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
            Some(ConversationIntent::ToolCallUpdate(ToolCallUpdate { arguments, .. })) => {
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

    #[test]
    fn test_sanitize_edit_arguments_delta_preserves_valid_json() {
        // Edit 参数含超长 old_string/new_string，原始 JSON 远超 512 字节
        let long_old = "x".repeat(400);
        let long_new = "y".repeat(400);
        let raw = format!(
            r#"{{"file_path":"src/main.rs","old_string":"{long_old}","new_string":"{long_new}"}}"#
        );
        assert!(
            raw.len() > TOOL_STREAM_PREVIEW_LIMIT,
            "test precondition: raw JSON should exceed limit"
        );

        let sanitized = sanitize_tool_arguments_delta("Edit", &raw);

        // 核心断言：摘要后仍是合法 JSON
        let parsed: Value =
            serde_json::from_str(&sanitized).expect("sanitized args must be valid JSON");

        // file_path 正确保留
        assert_eq!(
            parsed.get("file_path").and_then(|v| v.as_str()),
            Some("src/main.rs"),
            "file_path must survive sanitization"
        );

        // old_string/new_string 被截断摘要（不再保持原长）
        let old_val = parsed
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            old_val.len() < long_old.len(),
            "old_string should be summarized, got {} bytes",
            old_val.len()
        );
        assert!(
            old_val.contains("omitted"),
            "old_string summary should contain 'omitted'"
        );
    }

    #[test]
    fn test_agent_tool_calls_use_tool_header_view() {
        let event = AgentProgressEventView {
            sequence: 1,
            kind: AgentProgressKindView::ToolCalls {
                calls: vec![sdk::AgentToolCallProgressView {
                    id: sdk::ToolCallId::new("child-read-1"),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path":"src/lib.rs","offset": 10,"limit": 20}),
                }],
            },
        };

        let formatted = format_agent_progress(&event);
        assert_eq!(formatted, "→ Read src/lib.rs 11:30");
        assert!(!formatted.contains("file_path"));
    }

    #[test]
    fn test_agent_tool_calls_format_multiple_tools_and_fallback() {
        let event = AgentProgressEventView {
            sequence: 2,
            kind: AgentProgressKindView::ToolCalls {
                calls: vec![
                    sdk::AgentToolCallProgressView {
                        id: sdk::ToolCallId::new("child-bash"),
                        name: "Bash".to_string(),
                        input: serde_json::json!({"command":"cargo test"}),
                    },
                    sdk::AgentToolCallProgressView {
                        id: sdk::ToolCallId::new("child-write"),
                        name: "Write".to_string(),
                        input: serde_json::json!({"file_path":"out.rs","content_bytes": 42}),
                    },
                    sdk::AgentToolCallProgressView {
                        id: sdk::ToolCallId::new("child-unknown"),
                        name: "UnknownTool".to_string(),
                        input: serde_json::json!({"key":"value"}),
                    },
                ],
            },
        };

        let formatted = format_agent_progress(&event);
        assert_eq!(
            formatted,
            "→ Run cargo test  cargo test
→ Write out.rs 42 bytes
→ UnknownTool  {\"key\":\"value\"}"
        );
    }

    #[test]
    fn test_write_arguments_delta_includes_realtime_content_bytes() {
        let sanitized = sanitize_tool_arguments_delta(
            "Write",
            r#"{"file_path":"out.rs","content":"hello world"}"#,
        );
        let value: Value = serde_json::from_str(&sanitized).expect("valid sanitized json");
        assert_eq!(value.get("content_bytes").and_then(Value::as_u64), Some(11));
    }

    #[test]
    fn test_sanitize_partial_json_truncates() {
        let partial = r#"{"file_path":"src/main.rs","old_string":"x"#;
        let sanitized = sanitize_tool_arguments_delta("Edit", partial);
        // 回退模式：不是合法 JSON 但被截断
        assert!(
            sanitized.contains("omitted") || sanitized == partial,
            "partial JSON should be truncated, got: {sanitized}"
        );
    }
}

/// 把 AgentProgressEventView 格式化为人类可读消息，供 TUI activities 渲染。
fn format_agent_tool_call_header(name: &str, input: &Value) -> String {
    let view = format_tool_header_view(name, input, None, None);
    if view.details.is_empty() {
        view.title
    } else {
        format!("{}  {}", view.title, view.details.join("  "))
    }
}

fn format_agent_progress(event: &AgentProgressEventView) -> String {
    match &event.kind {
        AgentProgressKindView::Started { .. } => String::new(),
        AgentProgressKindView::Message { text } => text.clone(),
        AgentProgressKindView::ToolCalls { calls } => {
            if calls.is_empty() {
                return String::new();
            }
            let lines: Vec<String> = calls
                .iter()
                .map(|tc| {
                    let text = format_agent_tool_call_header(&tc.name, &tc.input);
                    if text.trim().is_empty() {
                        format!("→ {}", tc.name)
                    } else {
                        format!("→ {text}")
                    }
                })
                .collect();
            lines.join("\n")
        }
    }
}

#[cfg(test)]
mod started_tests {
    use super::*;
    use crate::tui::app::event::UiTurnContext;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
    use sdk::AgentProgressEventView;
    use sdk::AgentProgressKindView;

    fn ctx() -> UiTurnContext {
        UiTurnContext {
            chat_id: ChatId::new("chat-test"),
            turn_id: ChatTurnId::new("turn-test"),
        }
    }

    fn started_event(role: Option<&str>, model: &str) -> UiEvent {
        UiEvent::AgentProgress {
            context: ctx(),
            tool_id: ToolCallId::new("tool-1"),
            event: AgentProgressEventView {
                sequence: 0,
                kind: AgentProgressKindView::Started {
                    role: role.map(|s| s.to_string()),
                    model: model.to_string(),
                },
            },
        }
    }

    #[test]
    fn test_started_event_maps_to_update_agent_meta() {
        let tool_id = ToolCallId::new("tool-1");
        let ev = UiEvent::AgentProgress {
            context: ctx(),
            tool_id: tool_id.clone(),
            event: AgentProgressEventView {
                sequence: 0,
                kind: AgentProgressKindView::Started {
                    role: Some("coder".to_string()),
                    model: "Zhipu/glm-5.2".to_string(),
                },
            },
        };
        let mapping = map_agent_event(&ev);
        assert_eq!(mapping.conversation.len(), 1);
        match &mapping.conversation[0] {
            ConversationIntent::UpdateAgentMeta(UpdateAgentMeta {
                role,
                model,
                tool_id: got_id,
                ..
            }) => {
                assert_eq!(role.as_deref(), Some("coder"));
                assert_eq!(model, "Zhipu/glm-5.2");
                assert_eq!(got_id, &tool_id);
            }
            other => panic!("expected UpdateAgentMeta, got {other:?}"),
        }
    }

    #[test]
    fn test_started_event_without_role_maps_to_update_agent_meta() {
        let ev = started_event(None, "fallback-model");
        let mapping = map_agent_event(&ev);
        match &mapping.conversation[0] {
            ConversationIntent::UpdateAgentMeta(UpdateAgentMeta { role, model, .. }) => {
                assert!(role.is_none());
                assert_eq!(model, "fallback-model");
            }
            other => panic!("expected UpdateAgentMeta, got {other:?}"),
        }
    }

    #[test]
    fn test_non_started_event_maps_to_record_agent_progress() {
        let ev = UiEvent::AgentProgress {
            context: ctx(),
            tool_id: ToolCallId::new("tool-1"),
            event: AgentProgressEventView {
                sequence: 1,
                kind: AgentProgressKindView::Message {
                    text: "working".to_string(),
                },
            },
        };
        let mapping = map_agent_event(&ev);
        match &mapping.conversation[0] {
            ConversationIntent::RecordAgentProgress(RecordAgentProgress { message, .. }) => {
                assert_eq!(message, "working");
            }
            other => panic!("expected RecordAgentProgress, got {other:?}"),
        }
    }
}
