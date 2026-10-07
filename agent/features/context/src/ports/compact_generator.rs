//! Compact LLM 生成端口（driven port）——compact 逻辑抵达 LLM 的唯一边界。

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::domain::{CompactGenerationFailureData, CompactGenerationOutputData};

/// The implementor (typically a runtime adapter wrapping a `ProviderPort`)
/// invokes the LLM with the supplied request messages and returns the full
/// text output. Context retains ownership of prompt construction, map-reduce
/// orchestration, summary parsing, cancellation propagation, and fallback.
///
/// This trait is the sole boundary through which compact logic reaches an LLM;
/// production code must not depend on a concrete Provider construction handle.
#[async_trait]
pub trait CompactGenerator: Send + Sync {
    /// Generate the full text output for the given request messages.
    ///
    /// Returns the concatenated text content on success, or an error message
    /// describing the failure. Cancellation is cooperative via `cancel`.
    async fn generate(
        &self,
        request: Vec<Message>,
        cancel: &CancellationToken,
    ) -> Result<CompactGenerationOutputData, CompactGenerationFailureData>;

    /// 本次 compact 调用模型的输入窗口。
    ///
    /// `None`（默认）表示与 summary 注入的对话模型窗口一致；实现方只有在
    /// 确实使用不同模型时才返回更小窗口，Context 据此收紧 Map 分块目标，
    /// **NEVER** 依据未知窗口放大预算。
    async fn compact_context_window(&self) -> Option<usize> {
        None
    }
}
