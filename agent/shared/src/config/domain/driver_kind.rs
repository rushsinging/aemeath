//! Provider driver 身份词表（唯一真相源）。
//!
//! driver 字符串是跨 BC 的身份语义（#1850：身份与 UA 路由强耦合），
//! 词表 MUST 单点定义——provider 的 `ProviderDriverKind` 与各处
//! driver 字符串（config catalog、env 映射）都以本枚举为准。

use serde::{Deserialize, Serialize};

/// Provider driver 身份。每个 config 模型源的 `driver` 字段映射到其一。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DriverKind {
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

impl DriverKind {
    /// Parse from a config string.
    pub fn parse(s: &str) -> Option<DriverKind> {
        match s {
            "anthropic" => Some(DriverKind::Anthropic),
            "openai" => Some(DriverKind::OpenAI),
            "zhipu" => Some(DriverKind::Zhipu),
            "litellm" => Some(DriverKind::LiteLLM),
            "volcengine" => Some(DriverKind::Volcengine),
            "minimax" => Some(DriverKind::Minimax),
            "mimo" => Some(DriverKind::Mimo),
            "deepseek" => Some(DriverKind::DeepSeek),
            "agnes" => Some(DriverKind::Agnes),
            "ollama" => Some(DriverKind::Ollama),
            _ => None,
        }
    }

    /// Canonical config string.
    pub fn as_str(&self) -> &'static str {
        match self {
            DriverKind::Anthropic => "anthropic",
            DriverKind::OpenAI => "openai",
            DriverKind::Zhipu => "zhipu",
            DriverKind::LiteLLM => "litellm",
            DriverKind::Volcengine => "volcengine",
            DriverKind::Minimax => "minimax",
            DriverKind::Mimo => "mimo",
            DriverKind::DeepSeek => "deepseek",
            DriverKind::Agnes => "agnes",
            DriverKind::Ollama => "ollama",
        }
    }

    /// 全部已知 driver（一致性测试与目录校验用）。
    pub const ALL: [DriverKind; 10] = [
        DriverKind::Anthropic,
        DriverKind::OpenAI,
        DriverKind::Zhipu,
        DriverKind::LiteLLM,
        DriverKind::Volcengine,
        DriverKind::Minimax,
        DriverKind::Mimo,
        DriverKind::DeepSeek,
        DriverKind::Agnes,
        DriverKind::Ollama,
    ];
}

#[cfg(test)]
#[path = "driver_kind_tests.rs"]
mod tests;
