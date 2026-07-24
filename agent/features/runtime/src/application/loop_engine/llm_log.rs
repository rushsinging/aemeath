//! 统一 LLM 日志——Main 和 Sub 共用。
//!
//! 合并旧 `main_loop/looping/llm_log.rs` 和 `subagent/runner/logging.rs`。
//! schema 以 Main 版本为基准（更完整），Sub 调用时传入 `role` 参数。

use crate::application::main_loop::logged_input_messages;
use crate::application::main_loop::looping::InvocationResponse;
use crate::application::subagent::ToolCall;
use provider::RequestSystemBlock;
use sdk::ids::ToolCallId;
use share::message::Message;
use std::collections::HashMap;

/// 记录 LLM 输入日志。
///
/// `persisted_message_count`：已持久化消息数（Main 从 context 提取；Sub 传 0 即可）。
/// `role`：日志角色名（如 `"main"`、`"subagent:coder"`）。
pub(crate) fn log_llm_input(
    messages_for_api: &[Message],
    persisted_message_count: usize,
    system_blocks: &[RequestSystemBlock],
    tool_schemas: &[serde_json::Value],
    role: &str,
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
        "role": role,
        "messages": new_msgs,
        "system_blocks_count": system_blocks.len(),
        "system_blocks": sb_summary,
        "tool_schemas_count": tool_schemas.len(),
        "tool_schemas_names": schema_names,
    });
    log::debug!(
        target: crate::LOG_TARGET,
        "{}",
        serde_json::to_string(&data).unwrap_or_default()
    );
}

/// 构造 tool_call 日志数据。
pub(crate) fn build_tool_call_log(tool_call: &ToolCall, role: &str) -> serde_json::Value {
    serde_json::json!({
        "event_type": "tool_call",
        "role": role,
        "tool_use_id": tool_call.id,
        "tool_name": tool_call.name,
        "input": tool_call.input,
    })
}

/// 记录 tool_call 日志。
pub(crate) fn log_tool_calls(tool_calls: &[ToolCall], role: &str) {
    for tc in tool_calls {
        let tc_data = build_tool_call_log(tc, role);
        log::debug!(
            target: crate::LOG_TARGET,
            "tool_call: {}",
            serde_json::to_string(&tc_data).unwrap_or_default()
        );
    }
}

/// 构造 LLM 输出日志数据。
pub(crate) fn build_llm_output_log(
    provider_name: &str,
    resp: &InvocationResponse,
    api_elapsed: f64,
    role: &str,
) -> serde_json::Value {
    let blocks: Vec<serde_json::Value> = resp
        .assistant_message
        .content
        .iter()
        .filter_map(|block| serde_json::to_value(block).ok())
        .collect();
    serde_json::json!({
        "event_type": "llm_output",
        "role": role,
        "stop_reason": format!("{:?}", resp.stop_reason),
        "input_tokens": resp.usage.input_tokens.unwrap_or(0),
        "output_tokens": resp.usage.output_tokens.unwrap_or(0),
        "elapsed_secs": api_elapsed,
        "provider": provider_name,
        "content_blocks": blocks,
    })
}

/// 记录 LLM 输出 + tool_call 日志。
pub(crate) fn log_llm_output_and_tool_calls(
    provider_name: &str,
    resp: &InvocationResponse,
    tool_calls: &[ToolCall],
    api_elapsed: f64,
    role: &str,
) {
    let data = build_llm_output_log(provider_name, resp, api_elapsed, role);
    log::debug!(
        target: crate::LOG_TARGET,
        "{}",
        serde_json::to_string(&data).unwrap_or_default()
    );

    log_tool_calls(tool_calls, role);
}
/// 构造已知工具名的 tool_result 日志数据。
pub(crate) fn build_named_tool_result_log(
    id: &ToolCallId,
    tool_name: &str,
    output: &str,
    is_error: bool,
    role: &str,
) -> serde_json::Value {
    serde_json::json!({
        "event_type": "tool_result",
        "role": role,
        "tool_use_id": id,
        "tool_name": tool_name,
        "is_error": is_error,
        "output": output,
    })
}

/// 构造 tool_result 日志数据。
pub(crate) fn build_tool_result_log(
    id: &ToolCallId,
    output: &str,
    is_error: bool,
    call_info: &HashMap<ToolCallId, (String, String)>,
    role: &str,
) -> serde_json::Value {
    let tool_name = call_info
        .get(id)
        .map(|(name, _)| name.as_str())
        .unwrap_or("?");
    build_named_tool_result_log(id, tool_name, output, is_error, role)
}

/// 记录 tool_result 日志。
pub(crate) fn log_tool_result(
    id: &ToolCallId,
    output: &str,
    is_error: bool,
    call_info: &HashMap<ToolCallId, (String, String)>,
    role: &str,
) {
    let data = build_tool_result_log(id, output, is_error, call_info, role);
    log::debug!(
        target: crate::LOG_TARGET,
        "tool_result: {}",
        serde_json::to_string(&data).unwrap_or_default()
    );
}
