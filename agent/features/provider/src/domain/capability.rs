//! Provider driver capability.

use serde::{Deserialize, Serialize};
pub use share::reasoning::ReasoningLevel;

use crate::published_language::{ReasoningCapabilityData, ReasoningMappingKindData};

/// 由客户端上报的最大推理档位构造推理能力：`Off..=max` 全部支持。
///
/// 组合根装配 capability 时的唯一阶梯推导（自 composition 收编）；
/// mapping 暂固定 Effort，随 ReasoningMappingKindData 消费化处置调整。
pub fn reasoning_capability_from_max(max: ReasoningLevel) -> ReasoningCapabilityData {
    let all_levels = [
        ReasoningLevel::Off,
        ReasoningLevel::Minimal,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
        ReasoningLevel::Xhigh,
        ReasoningLevel::Max,
    ];
    let supported: Vec<_> = all_levels
        .into_iter()
        .filter(|level| *level <= max)
        .collect();
    ReasoningCapabilityData::new(supported, ReasoningMappingKindData::Effort)
        .unwrap_or_else(|_| ReasoningCapabilityData::none())
}

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
