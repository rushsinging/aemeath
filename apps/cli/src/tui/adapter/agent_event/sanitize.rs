use crate::tui::text::safe_str_slice_by_char;
use serde_json::{Map, Value};

// ════════════════════════════════════════════════════════════════════
//  Helpers — tool output sanitization (inlined from tool_flow_projector)
// ════════════════════════════════════════════════════════════════════

const TOOL_TEXT_PREVIEW_LIMIT: usize = 16 * 1024;
pub(super) const TOOL_STREAM_PREVIEW_LIMIT: usize = 512;
const TOOL_LARGE_FIELD_PREVIEW_LIMIT: usize = 256;

pub(super) fn json_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub(super) fn sanitize_tool_arguments_delta(tool_name: &str, partial_args: &str) -> String {
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

pub(super) fn sanitize_tool_output(tool_name: &str, output: &str) -> String {
    truncate_large_tool_text(output, Some(tool_name))
}

pub(super) fn sanitize_tool_result_content(tool_name: &str, content: Value) -> Value {
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
