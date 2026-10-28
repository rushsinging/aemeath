//! LLM 调用端口的 adapter 实现（#1863 引入文本消费、#2146 收敛为单端口）。
//!
//! 能力端口 `LlmInvokePort` 定义在 [`crate::published_language`]；
//! 本模块提供 `LlmClient` 的流式实现与测试用占位实现。
//! 文本消费是端口的默认方法（收口），不在本模块重复实现。

use async_trait::async_trait;

use crate::published_language::{
    LlmInvokePort, ModelInfo, ProviderError, ProviderRequestData, ProviderResponseStream,
};

#[async_trait]
impl LlmInvokePort for crate::adapters::client::LlmClient {
    async fn invoke_stream(
        &self,
        model: &ModelInfo,
        request: &ProviderRequestData,
    ) -> Result<ProviderResponseStream, ProviderError> {
        // 固有方法同名：UFCS 显式指向固有实现，避免递归进 trait 方法。
        crate::adapters::client::LlmClient::invoke_stream(self, model, request).await
    }
}

/// 占位调用端口（#2142）：恒返回 `ModelUnavailable`，供装配层测试 fake
/// 填充 `ProviderBindingData.provider`——不参与真实调用的测试路径零成本。
#[cfg(any(test, feature = "dev"))]
pub struct UnsupportedLlmInvokePort;

#[cfg(any(test, feature = "dev"))]
#[async_trait]
impl LlmInvokePort for UnsupportedLlmInvokePort {
    async fn invoke_stream(
        &self,
        _model: &ModelInfo,
        _request: &ProviderRequestData,
    ) -> Result<ProviderResponseStream, ProviderError> {
        Err(ProviderError::fatal(
            crate::published_language::ProviderErrorKind::ModelUnavailable,
            "占位调用端口（测试 fake）：本 binding 未装配真实 LLM 调用句柄",
        ))
    }
}

/// 测试 fake 快捷构造：`provider: provider::composition::unsupported_llm_invoke_port()`。
#[cfg(any(test, feature = "dev"))]
pub fn unsupported_llm_invoke_port() -> std::sync::Arc<dyn LlmInvokePort> {
    std::sync::Arc::new(UnsupportedLlmInvokePort)
}
