//! LLM Provider trait and common types

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

pub use crate::domain::capability::ReasoningLevel;
use crate::domain::invoke::{InvocationScope, SystemBlock};

/// Provider 内部旧 decoder 使用的事件接收器；不从 crate root 导出。
#[doc(hidden)]
pub trait LegacyStreamSink: Send {
    fn on_text(&mut self, text: &str);
    fn on_tool_use_start(&mut self, name: &str, provider_id: Option<&str>, index: usize);
    fn on_error(&mut self, error: &str);
    fn on_raw_line(&mut self, _line: &str) {}
    fn on_block_complete(&mut self, _full_text: &str) {}
    fn on_thinking(&mut self, _text: &str) {}
    fn on_tool_arguments_delta(
        &mut self,
        _index: usize,
        _name: &str,
        _provider_id: Option<&str>,
        _partial_args: &str,
    ) {
    }
}

/// LLM Provider trait - all providers must implement this
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 返回由 Runtime 主动 poll 的单请求事件流。
    async fn invocation_stream(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStream, crate::ProviderError> {
        let _ = (scope, system, messages, tool_schemas);
        if cancel.is_cancelled() {
            return Err(crate::ProviderError::cancelled());
        }
        Err(crate::ProviderError::fatal(
            crate::ProviderErrorKind::Configuration,
            "provider test double does not implement invocation_stream",
        ))
    }

    /// 仅供迁移期 decoder 与测试替身使用；生产入口必须使用 `invocation_stream`。
    #[doc(hidden)]
    async fn legacy_stream_message(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        sink: &mut dyn LegacyStreamSink,
        cancel: &CancellationToken,
    ) -> Result<crate::StreamResponse, crate::LlmError>;

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
mod tests {
    use super::ReasoningLevel;

    #[test]
    fn test_reasoning_level_ord_and_clamp() {
        assert!(ReasoningLevel::Low < ReasoningLevel::High);
        assert!(ReasoningLevel::High < ReasoningLevel::Max);
        assert_eq!(
            ReasoningLevel::Xhigh.clamped_to(ReasoningLevel::Medium),
            ReasoningLevel::Medium
        );
        assert_eq!(
            ReasoningLevel::Low.clamped_to(ReasoningLevel::High),
            ReasoningLevel::Low
        );
        assert_eq!(
            ReasoningLevel::Off.clamped_to(ReasoningLevel::Off),
            ReasoningLevel::Off
        );
    }

    #[test]
    fn test_reasoning_level_as_str() {
        assert_eq!(ReasoningLevel::Off.as_str(), "off");
        assert_eq!(ReasoningLevel::Low.as_str(), "low");
        assert_eq!(ReasoningLevel::Medium.as_str(), "medium");
        assert_eq!(ReasoningLevel::High.as_str(), "high");
        assert_eq!(ReasoningLevel::Xhigh.as_str(), "xhigh");
        assert_eq!(ReasoningLevel::Max.as_str(), "max");
    }

    #[test]
    fn test_reasoning_level_parse() {
        assert_eq!(ReasoningLevel::parse("high"), Some(ReasoningLevel::High));
        assert_eq!(ReasoningLevel::parse("HIGH"), Some(ReasoningLevel::High));
        assert_eq!(ReasoningLevel::parse("max"), Some(ReasoningLevel::Max));
        assert_eq!(ReasoningLevel::parse("invalid"), None);
        assert_eq!(ReasoningLevel::parse(""), None);
    }

    #[test]
    fn test_reasoning_level_display() {
        assert_eq!(format!("{}", ReasoningLevel::Medium), "medium");
        assert_eq!(format!("{}", ReasoningLevel::Xhigh), "xhigh");
    }

    #[test]
    fn test_reasoning_level_serde() {
        let json = serde_json::to_string(&ReasoningLevel::High).unwrap();
        assert_eq!(json, "\"high\"");
        let level: ReasoningLevel = serde_json::from_str("\"xhigh\"").unwrap();
        assert_eq!(level, ReasoningLevel::Xhigh);
    }
}
