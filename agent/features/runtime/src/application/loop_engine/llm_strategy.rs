//! LLM invocation strategy trait and shared helpers.
//!
//! The [`LlmStrategy`] trait abstracts per-adapter LLM invocation differences:
//! reasoning level (dynamic vs fixed), delta commitment (visible vs no-op sink),
//! and retry notification (event vs log).
//!
//! Shared free functions extract the two stable code blocks that are
//! character-identical between Main and Sub:
//! - [`extract_invocation_context`] maps ContextWindow → messages/system_blocks/tool_schemas
//! - [`build_step_token_usage`] constructs StepTokenUsage from InvocationResponse
//! - [`map_retry_outcome`] dispatches RetryStep → LoopEngineError (shared Compact/Fail,
//!   adapter-specific Retry/Cancelled via trait hooks)

use async_trait::async_trait;
use std::time::Duration;

use provider::RequestSystemBlock;
use share::message::Message;

use crate::application::loop_engine::{LoopEngineError, StepTokenUsage};
use crate::application::main_loop::looping::InvocationResponse;
use crate::application::model_invocation::RetryStep;
use crate::ports::{ContextWindow, ReasoningLevel};

/// Strategy for LLM invocation — hooks that differ between Main and Sub adapters.
///
/// Each adapter implements this trait directly on its adapter struct.
/// The trait is never used with `dyn` dispatch; callers use `impl LlmStrategy`
/// in generic free functions to avoid vtable indirection.
#[async_trait]
pub(crate) trait LlmStrategy {
    /// Reasoning level for this invocation.
    /// Main returns the current dynamic level; Sub returns its fixed level.
    fn reasoning_level(&self) -> ReasoningLevel;

    /// Whether stream deltas are committed to a user-visible sink.
    /// Main: `true` (deltas are projected to the chat UI).
    /// Sub: `false` (deltas go to a no-op sink so are safe to retry).
    fn committed_delta(&self) -> bool;

    /// Called when a retry is scheduled. Main emits `ModelInvocationRetrying`
    /// event; Sub logs via `log::info!`.
    async fn on_retry(&mut self, attempt: u32, delay: Duration);

    /// Called when a retry is cancelled (e.g. cancellation token fires during
    /// the retry delay). Default: no-op (Main has no extra cleanup). Sub
    /// overrides to propagate cancellation to the runtime token.
    async fn on_retry_cancelled(&mut self) {}
}

/// Output of [`extract_invocation_context`] — the three API invocation primitives
/// derived from a [`ContextWindow`].
pub(crate) struct InvocationContext {
    pub messages_for_api: Vec<Message>,
    pub tool_schemas: Vec<serde_json::Value>,
    pub system_blocks: Vec<RequestSystemBlock>,
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
        total_tokens: crate::application::token_usage::normalized_total_tokens(&resp.usage),
        context_window,
        est_system_tokens,
        est_tool_tokens,
        est_message_tokens,
        stop_reason: format!("{:?}", resp.stop_reason).to_lowercase(),
    }
}

/// Shared retry outcome dispatch.
///
/// Maps the four [`RetryStep`] variants to either loop-continuation (`Ok(())`)
/// or a [`LoopEngineError`] to propagate (`Err(...)`).  Retry and Cancelled
/// delegate to the corresponding [`LlmStrategy`] hooks; Compact and Fail are
/// adapter-agnostic.
pub(crate) async fn map_retry_outcome(
    step: RetryStep,
    error_string: &str,
    strategy: &mut (impl LlmStrategy + Send),
) -> Result<(), LoopEngineError> {
    match step {
        RetryStep::Retry { attempt, delay } => {
            strategy.on_retry(attempt, delay).await;
            Ok(())
        }
        RetryStep::Cancelled => {
            strategy.on_retry_cancelled().await;
            Err(LoopEngineError::Cancelled)
        }
        RetryStep::Compact => Err(LoopEngineError::NeedsCompaction(error_string.to_string())),
        RetryStep::Fail => Err(LoopEngineError::Adapter(error_string.to_string())),
    }
}
