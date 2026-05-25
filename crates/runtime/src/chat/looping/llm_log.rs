use crate::api::core::agent::ToolCall;
use crate::api::core::message::Message;
use crate::api::provider::types::{StreamResponse, SystemBlock};
use crate::api::storage::logging::JsonLogger;
use crate::chat::looping::input_log::logged_input_messages;
use std::sync::Arc;

pub(super) fn log_llm_input(
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    model_name: &str,
    messages_for_api: &[Message],
    persisted_message_count: usize,
    system_blocks: &[SystemBlock],
    tool_schemas: &[serde_json::Value],
) {
    let Some(jl) = json_logger else {
        return;
    };

    let new_msgs = logged_input_messages(messages_for_api, persisted_message_count);
    let sb_summary: Vec<serde_json::Value> = system_blocks
        .iter()
        .map(|sb| {
            serde_json::json!({
                "type": sb.block_type,
                "len": sb.text.len(),
            })
        })
        .collect();
    let schema_names: Vec<&str> = tool_schemas
        .iter()
        .map(|s| s.get("name").and_then(|v| v.as_str()).unwrap_or("?"))
        .collect();
    let data = serde_json::json!({
        "messages": new_msgs,
        "system_blocks_count": system_blocks.len(),
        "system_blocks": sb_summary,
        "tool_schemas_count": tool_schemas.len(),
        "tool_schemas_names": schema_names,
    });
    let _ = jl
        .lock()
        .unwrap()
        .log_input(turn_count, "default", model_name, data);
}

pub(super) fn log_llm_output_and_tool_calls(
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    provider_name: &str,
    model_name: &str,
    resp: &StreamResponse,
    tool_calls: &[ToolCall],
    api_elapsed: f64,
) {
    let Some(jl) = json_logger else {
        return;
    };

    let blocks: Vec<serde_json::Value> = resp
        .assistant_message
        .content
        .iter()
        .filter_map(|block| serde_json::to_value(block).ok())
        .collect();
    let data = serde_json::json!({
        "stop_reason": format!("{:?}", resp.stop_reason),
        "input_tokens": resp.usage.input_tokens,
        "output_tokens": resp.usage.output_tokens,
        "elapsed_secs": api_elapsed,
        "provider": provider_name,
        "content_blocks": blocks,
    });
    let _ = jl
        .lock()
        .unwrap()
        .log_output(turn_count, "default", model_name, data);

    for tc in tool_calls {
        let tc_data = serde_json::json!({
            "tool_use_id": tc.id,
            "tool_name": tc.name,
            "input": tc.input,
        });
        let _ = jl
            .lock()
            .unwrap()
            .log_tool_call(turn_count, "default", model_name, tc_data);
    }
}
