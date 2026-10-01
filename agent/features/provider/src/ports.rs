//! LLM Provider trait and common types

use async_trait::async_trait;
use share::message::Message;
pub(crate) use share::reasoning::ReasoningLevel;
use tokio_util::sync::CancellationToken;

use crate::domain::invoke::{InvocationScopeData, SystemBlockData};

/// LLM Provider trait - all providers must implement this
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 返回由 Runtime 主动 poll 的单请求事件流。
    async fn invocation_stream(
        &self,
        scope: &InvocationScopeData,
        system: &[SystemBlockData],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStreamData, crate::ProviderError>;

    /// Get the model name
    fn model_name(&self) -> &str;

    /// Get the provider name
    fn provider_name(&self) -> &str;

    /// 声明此 provider 支持的最高档位（graph 用于 clamp 决策）。
    /// 默认 High，各 driver 按能力覆盖。
    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::High
    }
}

#[cfg(test)]
#[path = "ports_tests.rs"]
mod tests;
