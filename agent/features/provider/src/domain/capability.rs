//! Provider driver capability.

use serde::{Deserialize, Serialize};
pub use share::reasoning::ReasoningLevel;

/// Provider driver kind. Every model source in config.json maps to one of these via its `driver` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
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
#[path = "capability_tests.rs"]
mod tests;
