//! LLM API driver kinds - core definitions for API protocol selection.
//!
//! The canonical model source list lives in config.json (`models.providers`).
//! This module only defines API driver types understood by code.

use serde::{Deserialize, Serialize};

/// API driver kind. Every model source in config.json maps to one of these via its `api` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiDriverKind {
    Anthropic,
    OpenAI,
    Zhipu,
    LiteLLM,
    Volcengine,
}

impl Default for ApiDriverKind {
    fn default() -> Self {
        ApiDriverKind::Anthropic
    }
}

impl ApiDriverKind {
    /// Parse from a config string.
    pub fn from_str(s: &str) -> Option<ApiDriverKind> {
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
        assert_eq!(
            ApiDriverKind::from_str("openai"),
            Some(ApiDriverKind::OpenAI)
        );
    }

    #[test]
    fn test_from_str_zhipu() {
        assert_eq!(ApiDriverKind::from_str("zhipu"), Some(ApiDriverKind::Zhipu));
    }

    #[test]
    fn test_from_str_litellm() {
        assert_eq!(
            ApiDriverKind::from_str("litellm"),
            Some(ApiDriverKind::LiteLLM)
        );
    }

    #[test]
    fn test_from_str_volcengine() {
        assert_eq!(
            ApiDriverKind::from_str("volcengine"),
            Some(ApiDriverKind::Volcengine)
        );
    }

    #[test]
    fn test_from_str_rejects_openai_compatible() {
        assert_eq!(ApiDriverKind::from_str("openai-compatible"), None);
        assert_eq!(ApiDriverKind::from_str("openai-completions"), None);
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
