use crate::api::agent::ToolCall;
use crate::api::core::message::Message;
use serde_json::json;

pub(crate) fn build_json_logger_input_data(
    messages: &[Message],
    system_blocks_count: usize,
    tool_schemas: &[serde_json::Value],
) -> serde_json::Value {
    let new_messages: Vec<serde_json::Value> = messages
        .get(messages.len().saturating_sub(1)..)
        .unwrap_or(&[])
        .iter()
        .map(|m| {
            let blocks: Vec<serde_json::Value> = m
                .content
                .iter()
                .filter_map(|b| serde_json::to_value(b).ok())
                .collect();
            json!({
                "role": m.role,
                "content_blocks": blocks,
                "block_count": m.content.len(),
            })
        })
        .collect();
    json!({
        "messages": new_messages,
        "system_blocks_count": system_blocks_count,
        "tool_schemas_count": tool_schemas.len(),
        "tool_schemas_names": tool_schemas.iter().map(|s| s.get("name").and_then(|v| v.as_str()).unwrap_or("?")).collect::<Vec<_>>(),
    })
}

pub(crate) fn build_json_logger_output_data(
    resp: &crate::api::provider::types::StreamResponse,
    elapsed_secs: f64,
    provider: &str,
) -> serde_json::Value {
    let blocks: Vec<serde_json::Value> = resp
        .assistant_message
        .content
        .iter()
        .filter_map(|block| serde_json::to_value(block).ok())
        .collect();
    json!({
        "stop_reason": format!("{:?}", resp.stop_reason),
        "input_tokens": resp.usage.input_tokens,
        "output_tokens": resp.usage.output_tokens,
        "elapsed_secs": elapsed_secs,
        "provider": provider,
        "content_blocks": blocks,
    })
}

pub(crate) fn build_json_logger_tool_call_data(tool_call: &ToolCall) -> serde_json::Value {
    json!({
        "tool_use_id": tool_call.id,
        "tool_name": tool_call.name,
        "input": tool_call.input,
    })
}

pub(crate) fn build_json_logger_tool_result_data(
    id: &str,
    output: &str,
    is_error: bool,
    call_info: &std::collections::HashMap<String, (String, String)>,
) -> serde_json::Value {
    let tool_name = call_info
        .get(id)
        .map(|(name, _)| name.as_str())
        .unwrap_or("?");
    json!({
        "tool_use_id": id,
        "tool_name": tool_name,
        "is_error": is_error,
        "output": output,
    })
}
