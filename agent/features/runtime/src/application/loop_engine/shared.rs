//! Loop Engine 共享逻辑——Main 和 Sub 完全一致的方法提取到此。

use crate::application::tool::agent::ToolExecution;
use crate::application::tool::result_materialization::ToolResultMaterializer;

/// Materialize a batch of [`ToolExecution`]s into a single [`Message`] with
/// tool-result content blocks, mapping through `provider_id`.
///
/// Shared tool-result materialization for every run kind. The Loop Engine owns
/// this path so Main and Sub adapters cannot diverge on wire IDs or persistence.
/// It maps each execution to `(provider_id, text, data, is_error, images)` and
/// then delegates to `materialize_provider_results`.
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
