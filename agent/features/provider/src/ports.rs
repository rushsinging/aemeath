//! LLM Provider trait and common types

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

pub use crate::domain::capability::ReasoningLevel;
use crate::domain::invoke::{InvocationScope, SystemBlock};

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
    ) -> Result<crate::InvocationStream, crate::ProviderError>;

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
        assert!(ReasoningLevel::Off < ReasoningLevel::Minimal);
        assert!(ReasoningLevel::Minimal < ReasoningLevel::Low);
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
        assert_eq!(
            ReasoningLevel::Minimal.clamped_to(ReasoningLevel::Off),
            ReasoningLevel::Off
        );
    }

    #[test]
    fn test_reasoning_level_as_str() {
        assert_eq!(ReasoningLevel::Off.as_str(), "off");
        assert_eq!(ReasoningLevel::Minimal.as_str(), "minimal");
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
        assert_eq!(
            ReasoningLevel::parse("minimal"),
            Some(ReasoningLevel::Minimal)
        );
        assert_eq!(
            ReasoningLevel::parse("MINIMAL"),
            Some(ReasoningLevel::Minimal)
        );
        assert_eq!(ReasoningLevel::parse("none"), Some(ReasoningLevel::Off));
        assert_eq!(ReasoningLevel::parse("NONE"), Some(ReasoningLevel::Off));
        assert_eq!(ReasoningLevel::parse("invalid"), None);
        assert_eq!(ReasoningLevel::parse(""), None);
    }

    #[test]
    fn test_reasoning_level_display() {
        assert_eq!(format!("{}", ReasoningLevel::Medium), "medium");
        assert_eq!(format!("{}", ReasoningLevel::Minimal), "minimal");
        assert_eq!(format!("{}", ReasoningLevel::Xhigh), "xhigh");
    }

    #[test]
    fn test_reasoning_level_serde() {
        let json = serde_json::to_string(&ReasoningLevel::High).unwrap();
        assert_eq!(json, "\"high\"");
        let level: ReasoningLevel = serde_json::from_str("\"xhigh\"").unwrap();
        assert_eq!(level, ReasoningLevel::Xhigh);
        // minimal round-trip
        let json = serde_json::to_string(&ReasoningLevel::Minimal).unwrap();
        assert_eq!(json, "\"minimal\"");
        let level: ReasoningLevel = serde_json::from_str("\"minimal\"").unwrap();
        assert_eq!(level, ReasoningLevel::Minimal);
        // none is NOT a canonical serialization value
        let err = serde_json::from_str::<ReasoningLevel>("\"none\"").unwrap_err();
        assert!(err.to_string().contains("none"));
    }
}
