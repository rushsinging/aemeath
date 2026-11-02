//! LLM 调用端口的 adapter 实现（#1863 引入文本消费、#2146 收敛为单端口）。
//!
//! 能力端口 `LlmInvokePort` 定义在 [`crate::published_language`]；
//! 本模块只提供 `LlmClient` 的流式实现——文本消费是端口的默认方法
//! （收口），不在本模块重复实现。

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
