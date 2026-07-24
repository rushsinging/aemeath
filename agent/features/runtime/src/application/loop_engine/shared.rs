//! RunLoopPort 共享逻辑——Main 和 Sub 完全一致的方法提取到此。

use super::LoopEngineError;
use crate::application::context_coordination::ContextCoordinator;
use crate::application::subagent::ToolExecution;
use crate::application::tool_result_materialization::ToolResultMaterializer;
use crate::ports::{CompactOutcome, ContextRequest, ContextWindow, SessionRevision};

/// 检查是否需要 compact，并返回最新 window。
///
/// Main 和 Sub 的 `needs_compaction` 实现字符级一致，提取至此。
/// 调用方需在调用后将返回的 window 存入 `self.context_window`。
pub(crate) async fn needs_compaction_with_window(
    context_request: Option<&ContextRequest>,
    context: &ContextCoordinator,
) -> Result<(bool, ContextWindow), LoopEngineError> {
    let request = context_request
        .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
    let window = context
        .build_window(request)
        .await
        .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
    let needed = context
        .needs_compaction(request)
        .await
        .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
    Ok((needed, window))
}

/// Materialize a batch of [`ToolExecution`]s into a single [`Message`] with
/// tool-result content blocks, mapping through `provider_id`.
///
/// Shared by Main (`tools::tool_results_for_api`) and Sub
/// (`loop_helpers::append_tool_results`). Previously both callers duplicated
/// the same `ToolExecution → (provider_id, text, data, is_error, images)`
/// mapping followed by `materialize_provider_results`.
pub(crate) async fn materialize_tool_results(
    materializer: &ToolResultMaterializer,
    results: Vec<ToolExecution>,
    session_id: &str,
) -> share::message::Message {
    let provider_results: Vec<_> = results
        .into_iter()
        .map(|ex| {
            (
                ex.provider_id,
                ex.outcome.text,
                ex.outcome.data,
                ex.outcome.is_error,
                ex.outcome.images,
            )
        })
        .collect();
    materializer
        .materialize_provider_results(session_id, provider_results)
        .await
}

/// Execute the core compact flow: validate inputs, invoke
/// [`ContextCoordinator::compact`], and apply the outcome via
/// [`apply_automatic_compact_outcome`].
///
/// The caller must extract `source_revision` from `context_window` before
/// calling this function to avoid a simultaneous immutable + mutable borrow
/// on the same field.
///
/// Returns the [`CompactOutcome`] so that callers (Main) can run additional
/// hooks (e.g. pre-compact snapshot + reflection) around the core. Sub
/// callers may ignore the return value.
pub(crate) async fn compact_core(
    context_request: Option<&ContextRequest>,
    source_revision: SessionRevision,
    context: &ContextCoordinator,
    last_total_tokens: &mut Option<u64>,
    context_window_out: &mut Option<ContextWindow>,
) -> Result<CompactOutcome, LoopEngineError> {
    let request = context_request
        .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
    let outcome = context
        .compact(request, source_revision)
        .await
        .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
    crate::application::context_coordination::apply_automatic_compact_outcome(
        &outcome,
        last_total_tokens,
        context_window_out,
    );
    Ok(outcome)
}
