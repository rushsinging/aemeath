//! 旧版 API 与模型配置
//!
//! 仅保留向后兼容。新配置应使用 `models` 字段。

use serde::{Deserialize, Serialize};

/// **Legacy** API configuration. Prefer using `models.providers` source entries instead.
///
/// This is kept for backward compatibility with existing config files and
/// commands (`/model`, `/config`). New configurations should use `ModelsConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Legacy LLM provider name (None = unset; Some = e.g. "anthropic", "deepseek").
    #[serde(default)]
    pub provider: Option<String>,

    /// API key (can also be set via driver-specific env var)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// API base URL (default: driver-specific)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// User agent string
    #[serde(default = "default_user_agent")]
    pub user_agent: String,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Number of retries for failed requests
    #[serde(default = "default_retries")]
    pub retries: u32,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            provider: None,
            key: None,
            base_url: None,
            user_agent: default_user_agent(),
            timeout: default_timeout(),
            retries: default_retries(),
        }
    }
}

pub(crate) fn default_user_agent() -> String {
    format!("aemeath/{}", crate::version())
}

pub(crate) fn default_timeout() -> u64 {
    // 保持与 provider 层 DEFAULT_TIMEOUT_SECS 一致。
    // shared crate 无法引用 provider 常量（依赖方向不允许），此处为镜像值。
    // 权威来源：agent/features/provider/src/business.rs
    1800
}

pub(crate) fn default_retries() -> u32 {
    3
}

/// **Legacy** model configuration. Prefer using `models.providers[].models[]` instead.
///
/// This is kept for backward compatibility. New configurations should define
/// models under source entries in `models.providers` in config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model name to use
    #[serde(default = "default_model")]
    pub name: String,

    /// Maximum output tokens
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Context window size
    #[serde(default = "default_context_size")]
    pub context_size: usize,

    /// Temperature (0.0 - 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Top-K sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,

    /// Top-P sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Stop sequences
    #[serde(default)]
    pub stop_sequences: Vec<String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            name: default_model(),
            max_tokens: default_max_tokens(),
            context_size: default_context_size(),
            temperature: None,
            top_k: None,
            top_p: None,
            stop_sequences: Vec::new(),
        }
    }
}

pub(crate) fn default_model() -> String {
    "claude-sonnet-4-6".to_string()
}

pub(crate) fn default_max_tokens() -> u32 {
    8192
}

pub(crate) fn default_context_size() -> usize {
    0
}
