use crate::render::{TerminalStreamHandler, ThinkingIndicator};
use aemeath_core::message::Message;
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

use super::context::build_user_context_message;

pub(super) async fn stream_next_response(
    client: &LlmClient,
    system_blocks: &[SystemBlock],
    messages: &[Message],
    user_context: &str,
    tool_schemas: &[serde_json::Value],
    cancel: &CancellationToken,
    verbose: bool,
    markdown: bool,
    json_logger: &Option<Arc<Mutex<aemeath_core::logging::JsonLogger>>>,
    turn_number: usize,
) -> Result<(aemeath_llm::types::StreamResponse, std::time::Duration), aemeath_llm::LlmError> {
    let mut messages_for_api = Vec::new();
    if let Some(ctx_msg) = build_user_context_message(user_context) {
        messages_for_api.push(ctx_msg);
    }
    messages_for_api.extend(messages.iter().cloned());

    let logged_messages =
        crate::tui::app::stream::logged_input_messages(&messages_for_api, messages.len());
    if let Some(jl) = json_logger {
        let data = serde_json::json!({
            "messages": logged_messages,
            "system_blocks_count": system_blocks.len(),
            "tool_schemas_count": tool_schemas.len(),
        });
        let _ = jl
            .lock()
            .unwrap()
            .log_input(turn_number, "default", client.model_name(), data);
    }

    let indicator = ThinkingIndicator::start("thinking...");
    let mut handler = TerminalStreamHandler::new(verbose, markdown);
    let response = client
        .stream_message(
            system_blocks,
            &messages_for_api,
            tool_schemas,
            &mut handler,
            cancel,
        )
        .await;
    let elapsed = indicator.elapsed();
    indicator.stop();
    response.map(|resp| (resp, elapsed))
}

pub(super) fn log_response(
    json_logger: &Option<Arc<Mutex<aemeath_core::logging::JsonLogger>>>,
    client: &LlmClient,
    turn_number: usize,
    resp: &aemeath_llm::types::StreamResponse,
    elapsed: std::time::Duration,
    tool_calls: &[aemeath_core::agent::ToolCall],
) {
    if let Some(jl) = json_logger {
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
            "elapsed_secs": elapsed.as_secs_f64(),
            "provider": client.provider_name(),
            "content_blocks": blocks,
        });
        let _ = jl
            .lock()
            .unwrap()
            .log_output(turn_number, "default", client.model_name(), data);
        for call in tool_calls {
            let data = serde_json::json!({"tool_use_id": call.id, "tool_name": call.name, "input": call.input});
            let _ =
                jl.lock()
                    .unwrap()
                    .log_tool_call(turn_number, "default", client.model_name(), data);
        }
    }
}
