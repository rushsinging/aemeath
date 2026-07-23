use crate::application::main_loop::looping::input_log::logged_input_messages;
use crate::application::main_loop::looping::stream_handler::InvocationResponse;
use crate::application::subagent::ToolCall;
use provider::RequestSystemBlock;
use share::message::Message;

/// 记录 LLM 输入到 `input.log`。
///
/// turn / model 等上下文由 `UnifiedLogger` 自动注入（无需在 payload 重复）。
pub(super) fn log_llm_input(
    messages_for_api: &[Message],
    persisted_message_count: usize,
    system_blocks: &[RequestSystemBlock],
    tool_schemas: &[serde_json::Value],
) {
    let new_msgs = logged_input_messages(messages_for_api, persisted_message_count);
    let sb_summary: Vec<serde_json::Value> = system_blocks
        .iter()
        .map(|sb| {
            serde_json::json!({
                "type": if sb.is_cacheable() { "cacheable" } else { "text" },
                "len": sb.text().len(),
            })
        })
        .collect();
    let schema_names: Vec<&str> = tool_schemas
        .iter()
        .map(|s| s.get("name").and_then(|v| v.as_str()).unwrap_or("?"))
        .collect();
    let data = serde_json::json!({
        "event_type": "llm_input",
        "messages": new_msgs,
        "system_blocks_count": system_blocks.len(),
        "system_blocks": sb_summary,
        "tool_schemas_count": tool_schemas.len(),
        "tool_schemas_names": schema_names,
    });
    log::debug!(target: crate::LOG_TARGET, "{}", serde_json::to_string(&data).unwrap_or_default());
}

/// 记录 LLM 完整输出 + tool_call 到 `output.log` / `tool.log`。
pub(super) fn log_llm_output_and_tool_calls(
    provider_name: &str,
    resp: &InvocationResponse,
    tool_calls: &[ToolCall],
    api_elapsed: f64,
) {
    let blocks: Vec<serde_json::Value> = resp
        .assistant_message
        .content
        .iter()
        .filter_map(|block| serde_json::to_value(block).ok())
        .collect();
    let data = serde_json::json!({
        "event_type": "llm_output",
        "stop_reason": format!("{:?}", resp.stop_reason),
        "input_tokens": resp.usage.input_tokens.unwrap_or(0),
        "output_tokens": resp.usage.output_tokens.unwrap_or(0),
        "elapsed_secs": api_elapsed,
        "provider": provider_name,
        "content_blocks": blocks,
    });
    log::debug!(target: crate::LOG_TARGET, "{}", serde_json::to_string(&data).unwrap_or_default());

    for tc in tool_calls {
        let tc_data = serde_json::json!({
            "tool_use_id": tc.id,
            "tool_name": tc.name,
            "input": tc.input,
        });
        log::debug!(
            target: crate::LOG_TARGET,
            "tool_call: {}",
            serde_json::to_string(&tc_data).unwrap_or_default()
        );
    }
}
