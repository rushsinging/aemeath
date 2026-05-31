//! Published language for the provider feature.
//!
//! This module exposes provider-domain DTOs and errors consumed by upstream
//! crates. LLM client/provider entry points live in `gateway`.

use serde::{Deserialize, Serialize};

pub use crate::business::types::{
    ApiError, CacheControl, ContentBlockPayload, CreateMessageRequest, DeltaPayload, DeltaUsage,
    MessageDeltaPayload, MessageStartPayload, StopReason, StreamEvent, StreamResponse,
    SystemBlock, Usage,
};
pub use crate::core::provider::LlmProvider;
pub use crate::LlmError;

/// Provider error alias for the published language.
pub type Error = LlmError;

/// OpenAI-compatible provider configuration DTOs.
pub mod openai_compatible {
    pub use crate::business::providers::openai_compatible::ReasoningConfig;
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
    Ollama,
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
            "ollama" => Some(ApiDriverKind::Ollama),
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
            ApiDriverKind::Ollama => "ollama",
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
    fn test_from_str_ollama() {
        assert_eq!(ApiDriverKind::parse("ollama"), Some(ApiDriverKind::Ollama));
    }

    #[test]
    fn test_as_str_ollama_roundtrip() {
        assert_eq!(ApiDriverKind::Ollama.as_str(), "ollama");
        assert_eq!(
            ApiDriverKind::parse(ApiDriverKind::Ollama.as_str()),
            Some(ApiDriverKind::Ollama)
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
