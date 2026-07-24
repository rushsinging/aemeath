//! RunLoopPort 共享逻辑——Main 和 Sub 完全一致的方法提取到此。

use super::LoopEngineError;
use crate::application::context_coordination::ContextCoordinator;
use crate::ports::{ContextRequest, ContextWindow};

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
