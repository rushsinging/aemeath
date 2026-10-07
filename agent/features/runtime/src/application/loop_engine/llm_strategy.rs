//! Shared helpers for model invocation orchestration.

#[cfg(test)]
#[path = "llm_strategy_tests.rs"]
mod tests;

use share::message::Message;

use crate::application::loop_engine::chat::InvocationResponse;
use crate::application::loop_engine::StepTokenUsage;
use crate::ports::ContextWindowData;

/// Output of [`extract_invocation_context`] — the three API invocation primitives
/// derived from a [`ContextWindowData`].
pub(crate) struct InvocationContext {
    messages_for_api: Vec<Message>,
    pub tool_schemas: Vec<serde_json::Value>,
    /// 拼好的整段 system prompt（块间 `\n\n` 连接——#1861 v4：块级
    /// 原块级 system 数据类型消除，拼接职责在此收口）。
    pub system: String,
    /// 可缓存前缀的字节长度：最后一个 `cache_break` 块末尾的累计字节数
    /// （0 = 无分界；分界天然落在块间，按字节切分 char-safe）。
    pub static_prefix_len: usize,
}

impl InvocationContext {
    /// 只读访问：映射后的窗口消息不可装饰（push/extend 等变更在编译期不可达）。
    pub(crate) fn messages_for_api(&self) -> &[Message] {
        &self.messages_for_api
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvocationMappingLogSummary {
    pub messages: usize,
    pub system_len: usize,
    pub tool_schemas: usize,
    pub reminder_messages: usize,
}

pub(crate) fn invocation_mapping_log_summary(
    invocation_context: &InvocationContext,
) -> InvocationMappingLogSummary {
    InvocationMappingLogSummary {
        messages: invocation_context.messages_for_api().len(),
        system_len: invocation_context.system.len(),
        tool_schemas: invocation_context.tool_schemas.len(),
        reminder_messages: invocation_context
            .messages_for_api
            .iter()
            .filter(|message| message.text_content().contains("<system-reminder>"))
            .count(),
    }
}

/// Map a [`ContextWindowData`] into the three invocation primitives:
/// LLM-visible messages, tool schema JSON objects, and the joined system prompt
/// (plus its cacheable-prefix byte boundary).
///
/// This logic is character-identical between Main and Sub.
pub(crate) fn extract_invocation_context(window: &ContextWindowData) -> InvocationContext {
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
    // 块映射改拼接：system = blocks.join("\n\n")；static_prefix_len = 最后一个
    // cache_break 块末尾的累计字节长度（含该块，不含其后的连接符）。
    let mut system = String::new();
    let mut static_prefix_len = 0usize;
    for (index, block) in window.system_blocks.iter().enumerate() {
        if index > 0 {
            system.push_str("\n\n");
        }
        system.push_str(&block.content);
        if block.cache_break {
            debug_assert!(block.cacheable, "cache breakpoint 必须位于可缓存前缀");
            static_prefix_len = system.len();
        }
    }
    InvocationContext {
        messages_for_api,
        tool_schemas,
        system,
        static_prefix_len,
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
