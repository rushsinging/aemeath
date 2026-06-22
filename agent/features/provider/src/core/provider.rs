//! LLM Provider trait and common types

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::business::types::{StreamResponse, SystemBlock};

/// 统一推理深度级别——所有 provider 的共同语言。
///
/// `Ord` derive 保证 clamp 语义：`desired.min(provider_max).min(user_max)`。
/// 各 provider 的实际档位能力不同，由 `max_reasoning_level()` 声明上限，
/// 超出时由调用方 clamp 到可用档位。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningLevel {
    /// 关闭 thinking
    Off,
    /// 浅度推理（省 token）
    Low,
    /// 中等
    Medium,
    /// 深度
    High,
    /// 超深度（GLM xhigh / DeepSeek max）
    Xhigh,
    /// 极限（GLM max）
    Max,
}

impl ReasoningLevel {
    /// 字符串表示，用于日志和调试。
    pub fn as_str(&self) -> &'static str {
        match self {
            ReasoningLevel::Off => "off",
            ReasoningLevel::Low => "low",
            ReasoningLevel::Medium => "medium",
            ReasoningLevel::High => "high",
            ReasoningLevel::Xhigh => "xhigh",
            ReasoningLevel::Max => "max",
        }
    }

    /// 从字符串解析，大小写不敏感。
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Some(ReasoningLevel::Off),
            "low" => Some(ReasoningLevel::Low),
            "medium" => Some(ReasoningLevel::Medium),
            "high" => Some(ReasoningLevel::High),
            "xhigh" => Some(ReasoningLevel::Xhigh),
            "max" => Some(ReasoningLevel::Max),
            _ => None,
        }
    }

    /// 将本级别 clamp 到 `max` 指定的上限。
    pub fn clamped_to(self, max: ReasoningLevel) -> ReasoningLevel {
        if self > max {
            max
        } else {
            self
        }
    }

    /// Discriminant as u8，用于 AtomicU8 存储。
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// 从 u8 discriminant 还原。
    pub fn from_u8(v: u8) -> ReasoningLevel {
        match v {
            0 => ReasoningLevel::Off,
            1 => ReasoningLevel::Low,
            2 => ReasoningLevel::Medium,
            3 => ReasoningLevel::High,
            4 => ReasoningLevel::Xhigh,
            _ => ReasoningLevel::Max,
        }
    }
}

impl std::fmt::Display for ReasoningLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

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

    /// 统一入口：设置推理深度。各 provider 覆盖此方法做自身映射。
    fn set_reasoning_level(&self, _level: ReasoningLevel) {}

    /// 当前推理深度（用于 save/restore）。
    /// 默认 Off，各 provider 按内部状态覆盖。
    fn current_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::Off
    }

    /// 声明此 provider 支持的最高档位（graph 用于 clamp 决策）。
    /// 默认 High，各 driver 按能力覆盖。
    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::High
    }

    /// 是否开启了推理（current_reasoning_level != Off 的便捷判断）。
    fn is_reasoning(&self) -> bool {
        !matches!(self.current_reasoning_level(), ReasoningLevel::Off)
    }

    /// Set max_tokens override at runtime. `0` means inherit/default and should be ignored.
    fn set_max_tokens(&self, _max_tokens: u32) {}

    /// Get current runtime max_tokens override. `0` means inherit/default.
    fn max_tokens(&self) -> u32 {
        0
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
