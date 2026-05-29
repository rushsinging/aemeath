//! Provider domain public API types.
//!
//! These types form the provider domain's public contract that
//! `runtime::api` exposes for upstream consumers.

use serde::{Deserialize, Serialize};

// Provider crate 对外公开门面：仅精确暴露 runtime 实际消费的类型与子模块，
// 内部模块（client/pool/provider/providers/stream/types）保持私有。
pub use crate::client::{LlmClient, LlmConfigOptions, OpenAIProviderConfig};
pub use crate::pool::LlmClientPool;
pub use crate::provider::{CallbackHandler, LlmProvider, StreamHandler};
pub use crate::types::{StopReason, StreamResponse, SystemBlock, Usage};

/// openai_compatible 子模块（consumer 需要 `ReasoningConfig` 等驱动配置类型）。
pub mod openai_compatible {
    pub use crate::providers::openai_compatible::*;
}

/// API driver kind. Every model source in config.json maps to one of these via its `api` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ApiDriverKind {
    #[default]
    Anthropic,
    OpenAI,
    Zhipu,
    LiteLLM,
    Volcengine,
}

impl ApiDriverKind {
    /// Parse from a config string.
    pub fn parse(s: &str) -> Option<ApiDriverKind> {
        match s {
            "anthropic" => Some(ApiDriverKind::Anthropic),
            "openai" => Some(ApiDriverKind::OpenAI),
            "zhipu" => Some(ApiDriverKind::Zhipu),
            "litellm" => Some(ApiDriverKind::LiteLLM),
            "volcengine" => Some(ApiDriverKind::Volcengine),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ApiDriverKind::Anthropic => "anthropic",
            ApiDriverKind::OpenAI => "openai",
            ApiDriverKind::Zhipu => "zhipu",
            ApiDriverKind::LiteLLM => "litellm",
            ApiDriverKind::Volcengine => "volcengine",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_openai() {
        assert_eq!(ApiDriverKind::parse("openai"), Some(ApiDriverKind::OpenAI));
    }

    #[test]
    fn test_from_str_zhipu() {
        assert_eq!(ApiDriverKind::parse("zhipu"), Some(ApiDriverKind::Zhipu));
    }

    #[test]
    fn test_from_str_litellm() {
        assert_eq!(
            ApiDriverKind::parse("litellm"),
            Some(ApiDriverKind::LiteLLM)
        );
    }

    #[test]
    fn test_from_str_volcengine() {
        assert_eq!(
            ApiDriverKind::parse("volcengine"),
            Some(ApiDriverKind::Volcengine)
        );
    }

    #[test]
    fn test_from_str_rejects_openai_compatible() {
        assert_eq!(ApiDriverKind::parse("openai-compatible"), None);
        assert_eq!(ApiDriverKind::parse("openai-completions"), None);
    }

    #[test]
    fn test_as_str_openai() {
        assert_eq!(ApiDriverKind::OpenAI.as_str(), "openai");
    }

    #[test]
    fn test_as_str_anthropic() {
        assert_eq!(ApiDriverKind::Anthropic.as_str(), "anthropic");
    }

    #[test]
    fn test_as_str_zhipu() {
        assert_eq!(ApiDriverKind::Zhipu.as_str(), "zhipu");
    }

    #[test]
    fn test_as_str_litellm() {
        assert_eq!(ApiDriverKind::LiteLLM.as_str(), "litellm");
    }

    #[test]
    fn test_as_str_volcengine() {
        assert_eq!(ApiDriverKind::Volcengine.as_str(), "volcengine");
    }
}
