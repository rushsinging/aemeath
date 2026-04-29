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
}

impl Default for ApiType {
    fn default() -> Self {
        ApiType::Anthropic
    }
}

impl ApiType {
    /// Parse from a config string ("anthropic" or "openai-completions")
    pub fn from_str(s: &str) -> Option<ApiType> {
        match s {
            "anthropic" => Some(ApiType::Anthropic),
            "openai-completions" => Some(ApiType::OpenAICompatible),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ApiType::Anthropic => "anthropic",
            ApiType::OpenAICompatible => "openai-completions",
        }
    }
}
