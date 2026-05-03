//! LLM API types - core definitions for API protocol selection
//!
//! The canonical provider list lives in config.json (`models.providers`).
//! This module only defines the two API protocol types.

use serde::{Deserialize, Serialize};

/// API protocol type. Every provider in config.json maps to one of these.
///
/// An `OpenAICompatible` provider uses `/chat/completions` — this covers
/// OpenAI, DeepSeek, Moonshot, Zhipu, DashScope, Ollama, OpenRouter, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiType {
    Anthropic,
    OpenAICompatible,
    Zhipu,
    LiteLLM,
}

impl Default for ApiType {
    fn default() -> Self {
        ApiType::Anthropic
    }
}

impl ApiType {
    /// Parse from a config string ("anthropic" or "openai").
    ///
    /// The legacy "openai-completions" spelling is accepted for compatibility
    /// when reading old config files, but new config should use "openai".
    pub fn from_str(s: &str) -> Option<ApiType> {
        match s {
            "anthropic" => Some(ApiType::Anthropic),
            "openai" | "openai-completions" => Some(ApiType::OpenAICompatible),
            "zhipu" => Some(ApiType::Zhipu),
            "litellm" => Some(ApiType::LiteLLM),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ApiType::Anthropic => "anthropic",
            ApiType::OpenAICompatible => "openai",
            ApiType::Zhipu => "zhipu",
            ApiType::LiteLLM => "litellm",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_openai() {
        assert_eq!(ApiType::from_str("openai"), Some(ApiType::OpenAICompatible));
    }

    #[test]
    fn test_from_str_legacy_openai_completions() {
        assert_eq!(
            ApiType::from_str("openai-completions"),
            Some(ApiType::OpenAICompatible)
        );
    }

    #[test]
    fn test_from_str_zhipu() {
        assert_eq!(ApiType::from_str("zhipu"), Some(ApiType::Zhipu));
    }

    #[test]
    fn test_from_str_litellm() {
        assert_eq!(ApiType::from_str("litellm"), Some(ApiType::LiteLLM));
    }

    #[test]
    fn test_from_str_unknown() {
        assert_eq!(ApiType::from_str("unknown"), None);
    }

    #[test]
    fn test_as_str_openai() {
        assert_eq!(ApiType::OpenAICompatible.as_str(), "openai");
    }

    #[test]
    fn test_as_str_anthropic() {
        assert_eq!(ApiType::Anthropic.as_str(), "anthropic");
    }

    #[test]
    fn test_as_str_zhipu() {
        assert_eq!(ApiType::Zhipu.as_str(), "zhipu");
    }

    #[test]
    fn test_as_str_litellm() {
        assert_eq!(ApiType::LiteLLM.as_str(), "litellm");
    }
}
