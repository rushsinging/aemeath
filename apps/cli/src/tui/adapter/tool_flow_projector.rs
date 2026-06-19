use crate::tui::adapter::agent_event::AgentEventMapping;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;
use crate::tui::model::runtime_observation::RuntimeObservation;
use crate::tui::text::safe_str_slice_by_char;
use serde_json::{Map, Value};

const TOOL_TEXT_PREVIEW_LIMIT: usize = 16 * 1024;
const TOOL_STREAM_PREVIEW_LIMIT: usize = 512;
const TOOL_LARGE_FIELD_PREVIEW_LIMIT: usize = 256;

#[derive(Debug, Default)]
pub struct ToolFlowProjector;

impl ToolFlowProjector {
    pub fn project(observation: &RuntimeObservation) -> AgentEventMapping {
        match observation {
            RuntimeObservation::AssistantText { context, text } => {
                let mut mapping = conversation(ConversationIntent::ObserveAssistantText {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    text: text.clone(),
                });
                mapping
                    .runtime
                    .push(RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Generating));
                mapping
            }
            RuntimeObservation::ThinkingText { context, text } => {
                let mut mapping = conversation(ConversationIntent::ObserveThinkingText {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    text: text.clone(),
                });
                mapping
                    .runtime
                    .push(RuntimeIntent::SetSpinnerPhase(SpinnerPhase::Thinking));
                mapping
            }
            RuntimeObservation::BlockComplete { context } => {
                conversation(ConversationIntent::CompleteBlock {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                })
            }
            RuntimeObservation::ToolCallStart {
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
                conversation(ConversationIntent::ObserveToolCallStart {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    name: name.clone(),
                    index: *index,
                })
            }
            RuntimeObservation::ToolCallUpdate {
                context,
                id,
                provider_id,
                name,
                index,
                arguments,
                status,
            } => {
                crate::tui::log_trace!(
                    "map tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} ",
                    context.chat_id,
                    context.turn_id,
                    id,
                    provider_id,
                    name,
                    index,
                    status,
                    arguments.as_ref().map(|value| value.len()).unwrap_or(0),
                );
                conversation(ConversationIntent::ObserveToolCallUpdate {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    name: name.clone(),
                    index: *index,
                    arguments: arguments
                        .as_ref()
                        .map(|value| sanitize_tool_arguments_delta(name, value)),
                    status: *status,
                })
            }
            RuntimeObservation::ToolResult {
                context,
                id,
                provider_id,
                tool_name,
                output,
                content,
                is_error,
                image_count,
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
                    image_count,
                );
                conversation(ConversationIntent::ObserveToolResult {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    tool_name: tool_name.clone(),
                    output: sanitize_tool_output(tool_name, output),
                    content: sanitize_tool_result_content(tool_name, content.clone()),
                    is_error: *is_error,
                    image_count: *image_count,
                })
            }
            RuntimeObservation::AgentProgress {
                context,
                tool_id,
                message,
            } => conversation(ConversationIntent::RecordAgentProgress {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                tool_id: tool_id.clone(),
                message: message.clone(),
            }),
            RuntimeObservation::Complete { context } => {
                conversation(ConversationIntent::CompleteChat {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                })
            }
        }
    }
}

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
        "{} ... ({} bytes omitted from TUI {tool_name}.{field} preview)",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
    use crate::tui::model::conversation::tool_call::ToolCallStatus;
    use crate::tui::model::runtime_observation::RuntimeTurnContext;

    fn ctx() -> RuntimeTurnContext {
        RuntimeTurnContext::new(ChatId::new("chat-test"), ChatTurnId::new("turn-test"))
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
    fn test_projector_runtime_observations_do_not_emit_bind_runtime_turn() {
        let context = ctx();
        for observation in [
            RuntimeObservation::AssistantText {
                context: context.clone(),
                text: "hi".to_string(),
            },
            RuntimeObservation::ThinkingText {
                context: context.clone(),
                text: "hmm".to_string(),
            },
            RuntimeObservation::ToolCallStart {
                context: context.clone(),
                id: sdk::ids::ToolCallId::new("tool-1"),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
            },
            RuntimeObservation::ToolCallUpdate {
                context: context.clone(),
                id: sdk::ids::ToolCallId::new("tool-1"),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
                arguments: Some("{}".to_string()),
                status: ToolCallStatus::Ready,
            },
            RuntimeObservation::AgentProgress {
                context: context.clone(),
                tool_id: sdk::ids::ToolCallId::new("tool-1"),
                message: "running".to_string(),
            },
            RuntimeObservation::Complete {
                context: context.clone(),
            },
        ] {
            let mapping = ToolFlowProjector::project(&observation);
            assert_no_runtime_bind_prelude(&mapping);
        }
    }

    #[test]
    fn test_projector_preserves_tool_result_context() {
        let mapping = ToolFlowProjector::project(&RuntimeObservation::ToolResult {
            context: ctx(),
            id: sdk::ids::ToolCallId::new("tool-1"),
            provider_id: "provider-1".to_string(),
            tool_name: "Read".to_string(),
            output: "done".to_string(),
            content: serde_json::json!({ "text": "done" }),
            is_error: false,
            image_count: 0,
        });
        assert_no_runtime_bind_prelude(&mapping);
        let expected_id = sdk::ids::ToolCallId::new("tool-1");
        let expected_context = ctx();
        assert!(matches!(
            first_observation(&mapping),
            Some(ConversationIntent::ObserveToolResult { chat_id, turn_id, id, .. })
                if chat_id == &expected_context.chat_id && turn_id == &expected_context.turn_id && id == &expected_id
        ));
    }

    #[test]
    fn test_truncate_tool_text_preserves_utf8_boundary() {
        let text = "你好世界".repeat(20);
        let truncated = truncate_tool_text(&text, 31, None);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert!(truncated.contains("omitted"));
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
            "old_string should contain omission marker"
        );
    }

    #[test]
    fn test_sanitize_arguments_delta_fallback_on_partial_json() {
        // 流式传输中的不完整 JSON → 解析失败 → 回退到截断
        let partial = r#"{"file_path":"src/main.rs","old_string":"x"#;
        let sanitized = sanitize_tool_arguments_delta("Edit", partial);
        // 回退模式：不是合法 JSON 但被截断
        assert!(
            sanitized.contains("omitted") || sanitized == partial,
            "partial JSON should be truncated, got: {sanitized}"
        );
    }
}
