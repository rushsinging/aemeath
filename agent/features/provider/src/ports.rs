//! LLM Provider driver trait（crate 内部分派接口）与调用解析参数。

use async_trait::async_trait;
use share::message::Message;
pub(crate) use share::reasoning::ReasoningLevel;
use tokio_util::sync::CancellationToken;

/// driver 调用的解析参数（client.invoke 经能力 resolve 后的执行细节，
/// crate 私有——PL 只见 request/response）。
#[derive(Debug, Clone)]
pub(crate) struct ResolvedInvocation {
    /// provider 中性模型名。
    pub model: String,
    pub max_tokens: u32,
    pub requested_reasoning: ReasoningLevel,
    pub effective_reasoning: ReasoningLevel,
}

impl ResolvedInvocation {
    pub fn new(
        model: impl Into<String>,
        max_tokens: u32,
        requested_reasoning: ReasoningLevel,
        effective_reasoning: ReasoningLevel,
    ) -> Result<Self, crate::ProviderError> {
        let model = model.into();
        let configuration = |message: &str| {
            crate::ProviderError::fatal(crate::ProviderErrorKind::Configuration, message)
        };
        if model.trim().is_empty() {
            return Err(configuration("invocation model must not be empty"));
        }
        if max_tokens == 0 {
            return Err(configuration(
                "invocation max_tokens must be greater than zero",
            ));
        }
        if effective_reasoning > requested_reasoning {
            return Err(configuration(
                "effective reasoning must not exceed requested reasoning",
            ));
        }
        Ok(Self {
            model,
            max_tokens,
            requested_reasoning,
            effective_reasoning,
        })
    }
}

/// LLM Provider driver trait - all providers must implement this.
/// system 为整段拼好的 prompt；`static_prefix_len` 是可缓存前缀字节分界
/// （0 = 无），driver 内部各自转换 wire 形态。
#[async_trait]
pub(crate) trait LlmProvider: Send + Sync {
    /// 返回由 Runtime 主动 poll 的单请求事件流。
    async fn invocation_stream(
        &self,
        resolved: &ResolvedInvocation,
        system: &str,
        static_prefix_len: usize,
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::ProviderResponseStream, crate::ProviderError>;

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
