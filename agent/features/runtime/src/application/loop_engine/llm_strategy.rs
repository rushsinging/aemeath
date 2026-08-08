//! Shared helpers for model invocation orchestration.

#[cfg(test)]
#[path = "llm_strategy_tests.rs"]
mod tests;

use provider::RequestSystemBlock;
use share::message::Message;

use crate::application::loop_engine::chat::InvocationResponse;
use crate::application::loop_engine::StepTokenUsage;
use crate::ports::ContextWindow;

/// Output of [`extract_invocation_context`] — the three API invocation primitives
/// derived from a [`ContextWindow`].
pub(crate) struct InvocationContext {
    pub messages_for_api: Vec<Message>,
    pub tool_schemas: Vec<serde_json::Value>,
    pub system_blocks: Vec<RequestSystemBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvocationMappingLogSummary {
    pub messages: usize,
    pub system_blocks: usize,
    pub tool_schemas: usize,
    pub reminder_messages: usize,
}

pub(crate) fn invocation_mapping_log_summary(
    invocation_context: &InvocationContext,
) -> InvocationMappingLogSummary {
    InvocationMappingLogSummary {
        messages: invocation_context.messages_for_api.len(),
        system_blocks: invocation_context.system_blocks.len(),
        tool_schemas: invocation_context.tool_schemas.len(),
        reminder_messages: invocation_context
            .messages_for_api
            .iter()
            .filter(|message| message.text_content().contains("<system-reminder>"))
            .count(),
    }
}

/// Map a [`ContextWindow`] into the three invocation primitives:
/// LLM-visible messages, tool schema JSON objects, and provider system blocks.
///
/// This logic is character-identical between Main and Sub.
pub(crate) fn extract_invocation_context(window: &ContextWindow) -> InvocationContext {
    let messages_for_api = window
        .messages
        .iter()
        .map(Message::to_llm_view)
        .collect::<Vec<_>>();
    let tool_schemas = window
        .tool_schemas
        .iter()
        .map(|schema| schema.to_tool_definition())
        .collect::<Vec<_>>();
    let system_blocks = window
        .system_blocks
        .iter()
        .map(|block| {
            if block.cache_break {
                debug_assert!(block.cacheable, "cache breakpoint 必须位于可缓存前缀");
                RequestSystemBlock::Cacheable(block.content.clone())
            } else {
                RequestSystemBlock::Text(block.content.clone())
            }
        })
        .collect::<Vec<_>>();
    InvocationContext {
        messages_for_api,
        tool_schemas,
        system_blocks,
    }
}

/// Construct a [`StepTokenUsage`] from an [`InvocationResponse`] and token-estimation fields.
///
/// The field mapping is character-identical between Main and Sub; only the
/// source of `context_window` and `est_*_tokens` values differs.
pub(crate) fn build_step_token_usage(
    resp: &InvocationResponse,
    context_window: u64,
    est_system_tokens: usize,
    est_tool_tokens: usize,
    est_message_tokens: usize,
) -> StepTokenUsage {
    StepTokenUsage {
        input_tokens: resp.usage.input_tokens.unwrap_or(0) as u64,
        output_tokens: resp.usage.output_tokens.unwrap_or(0) as u64,
        cached_tokens: resp.usage.cache_read_tokens.map(u64::from).unwrap_or(0),
        cache_creation_tokens: resp.usage.cache_write_tokens.map(u64::from).unwrap_or(0),
        reasoning_tokens: resp.usage.reasoning_tokens.map(u64::from).unwrap_or(0),
        total_tokens: crate::application::model::token_usage::normalized_total_tokens(&resp.usage),
        context_window,
        est_system_tokens,
        est_tool_tokens,
        est_message_tokens,
        stop_reason: format!("{:?}", resp.stop_reason).to_lowercase(),
    }
}
