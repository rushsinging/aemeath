//! LLM Provider types - core definitions that can be used by both core and llm modules

use serde::{Deserialize, Serialize};

/// Supported LLM providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Anthropic,
    OpenAI,
    OpenRouter,
    DeepSeek,
    Moonshot,
    Zhipu,
    DashScope,
    MiniMax,
    /// Generic OpenAI-compatible provider
    OpenAICompatible,
    /// Ollama local inference server
    Ollama,
}

impl Provider {
    /// Parse provider from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Some(Provider::Anthropic),
            "openai" | "gpt" => Some(Provider::OpenAI),
            "openrouter" => Some(Provider::OpenRouter),
            "deepseek" => Some(Provider::DeepSeek),
            "moonshot" | "kimi" => Some(Provider::Moonshot),
            "zhipu" | "zhipuai" => Some(Provider::Zhipu),
            "dashscope" | "qwen" | "tongyi" => Some(Provider::DashScope),
            "minimax" => Some(Provider::MiniMax),
            "openai-compatible" | "compatible" => Some(Provider::OpenAICompatible),
            "ollama" => Some(Provider::Ollama),
            _ => None,
        }
    }

    /// Get default base URL for the provider
    pub fn default_base_url(&self) -> &'static str {
        match self {
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::OpenAI => "https://api.openai.com",
            Provider::OpenRouter => "https://openrouter.ai/api",
            Provider::DeepSeek => "https://api.deepseek.com",
            Provider::Moonshot => "https://api.moonshot.cn",
            Provider::Zhipu => "https://open.bigmodel.cn/api/paas/v4",
            Provider::DashScope => "https://dashscope.aliyuncs.com/compatible-mode",
            Provider::MiniMax => "https://api.minimaxi.com",
            Provider::OpenAICompatible => "",
            Provider::Ollama => "http://localhost:11434",
        }
    }

    /// Get default model for the provider
    pub fn default_model(&self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-sonnet-4-6",
            Provider::OpenAI => "gpt-4o",
            Provider::OpenRouter => "anthropic/claude-sonnet-4",
            Provider::DeepSeek => "deepseek-chat",
            Provider::Moonshot => "moonshot-v1-128k",
            Provider::Zhipu => "glm-4-plus",
            Provider::DashScope => "qwen-plus",
            Provider::MiniMax => "MiniMax-M1",
            Provider::OpenAICompatible => "",
            Provider::Ollama => "llama3.2",
        }
    }

    /// Get environment variable name for API key
    pub fn api_key_env(&self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
            Provider::DeepSeek => "DEEPSEEK_API_KEY",
            Provider::Moonshot => "MOONSHOT_API_KEY",
            Provider::Zhipu => "ZHIPU_API_KEY",
            Provider::DashScope => "DASHSCOPE_API_KEY",
            Provider::MiniMax => "MINIMAX_API_KEY",
            Provider::OpenAICompatible => "LLM_API_KEY",
            Provider::Ollama => "LLM_API_KEY",
        }
    }

    /// Get environment variable name for base URL
    pub fn base_url_env(&self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_BASE_URL",
            Provider::OpenAI => "OPENAI_BASE_URL",
            Provider::OpenRouter => "OPENROUTER_BASE_URL",
            Provider::DeepSeek => "DEEPSEEK_BASE_URL",
            Provider::Moonshot => "MOONSHOT_BASE_URL",
            Provider::Zhipu => "ZHIPU_BASE_URL",
            Provider::DashScope => "DASHSCOPE_BASE_URL",
            Provider::MiniMax => "MINIMAX_BASE_URL",
            Provider::OpenAICompatible => "LLM_BASE_URL",
            Provider::Ollama => "LLM_BASE_URL",
        }
    }

    /// Get the maximum output tokens supported by this provider (0 = no limit / use as-is)
    pub fn max_output_tokens(&self) -> u32 {
        match self {
            Provider::Anthropic => 8192,     // Claude models default max output
            Provider::OpenAI => 16384,       // GPT-4o max output
            Provider::OpenRouter => 4096,    // conservative default across many models
            Provider::DeepSeek => 8192,      // DeepSeek V3 max output
            Provider::Moonshot => 8192,      // Moonshot max output
            Provider::Zhipu => 4096,         // GLM-4 max output
            Provider::DashScope => 8192,     // Qwen max output
            Provider::MiniMax => 4096,       // MiniMax max output
            Provider::Ollama => 0,           // model-dependent, no hard limit
            Provider::OpenAICompatible => 0, // unknown, let config decide
        }
    }

    /// Get the chat completions API path suffix for OpenAI-compatible providers
    /// Some providers (like Zhipu) use different paths than the standard /v1/chat/completions
    pub fn chat_api_suffix(&self) -> &'static str {
        match self {
            Provider::Zhipu => "/chat/completions",
            Provider::Ollama => "/v1/chat/completions",
            _ => "/v1/chat/completions",
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::OpenAI => write!(f, "openai"),
            Provider::OpenRouter => write!(f, "openrouter"),
            Provider::DeepSeek => write!(f, "deepseek"),
            Provider::Moonshot => write!(f, "moonshot"),
            Provider::Zhipu => write!(f, "zhipu"),
            Provider::DashScope => write!(f, "dashscope"),
            Provider::MiniMax => write!(f, "minimax"),
            Provider::OpenAICompatible => write!(f, "openai-compatible"),
            Provider::Ollama => write!(f, "ollama"),
        }
    }
}