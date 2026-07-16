//! Provider driver capability.

use serde::{Deserialize, Serialize};

/// Provider driver kind. Every model source in config.json maps to one of these via its `driver` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProviderDriverKind {
    #[default]
    Anthropic,
    OpenAI,
    Zhipu,
    LiteLLM,
    Volcengine,
    Minimax,
    Mimo,
    DeepSeek,
    Agnes,
    Ollama,
}

impl ProviderDriverKind {
    /// Parse from a config string.
    pub fn parse(s: &str) -> Option<ProviderDriverKind> {
        match s {
            "anthropic" => Some(ProviderDriverKind::Anthropic),
            "openai" => Some(ProviderDriverKind::OpenAI),
            "zhipu" => Some(ProviderDriverKind::Zhipu),
            "litellm" => Some(ProviderDriverKind::LiteLLM),
            "volcengine" => Some(ProviderDriverKind::Volcengine),
            "minimax" => Some(ProviderDriverKind::Minimax),
            "mimo" => Some(ProviderDriverKind::Mimo),
            "deepseek" => Some(ProviderDriverKind::DeepSeek),
            "agnes" => Some(ProviderDriverKind::Agnes),
            "ollama" => Some(ProviderDriverKind::Ollama),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderDriverKind::Anthropic => "anthropic",
            ProviderDriverKind::OpenAI => "openai",
            ProviderDriverKind::Zhipu => "zhipu",
            ProviderDriverKind::LiteLLM => "litellm",
            ProviderDriverKind::Volcengine => "volcengine",
            ProviderDriverKind::Minimax => "minimax",
            ProviderDriverKind::Mimo => "mimo",
            ProviderDriverKind::DeepSeek => "deepseek",
            ProviderDriverKind::Agnes => "agnes",
            ProviderDriverKind::Ollama => "ollama",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_openai() {
        assert_eq!(
            ProviderDriverKind::parse("openai"),
            Some(ProviderDriverKind::OpenAI)
        );
    }

    #[test]
    fn test_from_str_zhipu() {
        assert_eq!(
            ProviderDriverKind::parse("zhipu"),
            Some(ProviderDriverKind::Zhipu)
        );
    }

    #[test]
    fn test_from_str_litellm() {
        assert_eq!(
            ProviderDriverKind::parse("litellm"),
            Some(ProviderDriverKind::LiteLLM)
        );
    }

    #[test]
    fn test_from_str_volcengine() {
        assert_eq!(
            ProviderDriverKind::parse("volcengine"),
            Some(ProviderDriverKind::Volcengine)
        );
    }

    #[test]
    fn test_from_str_minimax() {
        assert_eq!(
            ProviderDriverKind::parse("minimax"),
            Some(ProviderDriverKind::Minimax)
        );
    }

    #[test]
    fn test_from_str_ollama() {
        assert_eq!(
            ProviderDriverKind::parse("ollama"),
            Some(ProviderDriverKind::Ollama)
        );
    }

    #[test]
    fn test_from_str_mimo() {
        assert_eq!(
            ProviderDriverKind::parse("mimo"),
            Some(ProviderDriverKind::Mimo)
        );
    }

    #[test]
    fn test_as_str_ollama_roundtrip() {
        assert_eq!(ProviderDriverKind::Ollama.as_str(), "ollama");
        assert_eq!(
            ProviderDriverKind::parse(ProviderDriverKind::Ollama.as_str()),
            Some(ProviderDriverKind::Ollama)
        );
    }

    #[test]
    fn test_from_str_rejects_openai_compatible() {
        assert_eq!(ProviderDriverKind::parse("openai-compatible"), None);
        assert_eq!(ProviderDriverKind::parse("openai-completions"), None);
    }

    #[test]
    fn test_as_str_openai() {
        assert_eq!(ProviderDriverKind::OpenAI.as_str(), "openai");
    }

    #[test]
    fn test_as_str_anthropic() {
        assert_eq!(ProviderDriverKind::Anthropic.as_str(), "anthropic");
    }

    #[test]
    fn test_as_str_zhipu() {
        assert_eq!(ProviderDriverKind::Zhipu.as_str(), "zhipu");
    }

    #[test]
    fn test_as_str_litellm() {
        assert_eq!(ProviderDriverKind::LiteLLM.as_str(), "litellm");
    }

    #[test]
    fn test_as_str_minimax() {
        assert_eq!(ProviderDriverKind::Minimax.as_str(), "minimax");
    }

    #[test]
    fn test_as_str_mimo() {
        assert_eq!(ProviderDriverKind::Mimo.as_str(), "mimo");
    }

    #[test]
    fn test_as_str_volcengine() {
        assert_eq!(ProviderDriverKind::Volcengine.as_str(), "volcengine");
    }

    #[test]
    fn test_from_str_deepseek() {
        assert_eq!(
            ProviderDriverKind::parse("deepseek"),
            Some(ProviderDriverKind::DeepSeek)
        );
    }

    #[test]
    fn test_as_str_deepseek() {
        assert_eq!(ProviderDriverKind::DeepSeek.as_str(), "deepseek");
    }
}
