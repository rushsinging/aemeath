//! LLM Provider trait and common types

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

pub use crate::domain::capability::ReasoningLevel;
use crate::domain::invoke::{InvocationScope, StreamResponse, SystemBlock};

/// Handler trait for streaming responses
pub trait StreamHandler: Send {
    fn on_text(&mut self, text: &str);
    fn on_tool_use_start(&mut self, name: &str, provider_id: Option<&str>, index: usize);
    fn on_error(&mut self, error: &str);
    fn on_raw_line(&mut self, _line: &str) {}
    fn on_block_complete(&mut self, _full_text: &str) {}
    /// Called for reasoning/thinking content (e.g. GLM-5.1, DeepSeek-R1).
    /// Default: ignored. Override to display thinking in a special style.
    fn on_thinking(&mut self, _text: &str) {}
    /// Called when arguments delta arrives during streaming tool calls.
    /// `index` is the tool call index, `name` is the tool name,
    /// `provider_id` is the provider tool-use id when available,
    /// `partial_args` is the accumulated arguments string so far.
    fn on_tool_arguments_delta(
        &mut self,
        _index: usize,
        _name: &str,
        _provider_id: Option<&str>,
        _partial_args: &str,
    ) {
    }
}

/// Simple callback handler for raw text streaming
pub struct CallbackHandler {
    callback: Box<dyn FnMut(&str) + Send>,
}

impl CallbackHandler {
    pub fn new(callback: Box<dyn FnMut(&str) + Send>) -> Self {
        Self { callback }
    }
}

impl StreamHandler for CallbackHandler {
    fn on_text(&mut self, text: &str) {
        (self.callback)(text);
    }
    fn on_tool_use_start(&mut self, _name: &str, _provider_id: Option<&str>, _index: usize) {}
    fn on_error(&mut self, _error: &str) {}
    fn on_block_complete(&mut self, _full_text: &str) {}
}

/// LLM Provider trait - all providers must implement this
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream a message with tool support
    async fn stream_message(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError>;

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
