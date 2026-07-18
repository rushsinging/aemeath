//! 多来源模型配置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Multi-source model configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsConfig {
    /// Merge mode: "merge" to combine with env/CLI settings.
    #[serde(default)]
    pub mode: String,

    /// Default source and model in "<source>/<model>" format (e.g. "zhipu/glm-5.1").
    /// Used when no --model / AEMEATH_MODEL selection is set.
    #[serde(default)]
    pub default: String,

    /// Source configurations keyed by source key (stored in JSON as `models.providers`).
    #[serde(default)]
    pub providers: HashMap<String, ProviderModelsConfig>,

    /// Guidance file overrides, keyed by glob pattern (e.g. "zhipu/*" → "~/.aemeath/guidance/glm.md").
    #[serde(default)]
    pub guidance: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    pub source_key: String,
    pub source_config: ProviderModelsConfig,
    pub model: ModelEntryConfig,
    pub driver: String,
}

/// Configuration for a single model source within models config.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProviderModelsConfig {
    /// Base URL for the source API.
    #[serde(default, rename = "baseUrl")]
    pub base_url: String,

    /// API key for this source.
    #[serde(default, rename = "apiKey")]
    pub api_key: String,

    /// Provider driver: "openai", "anthropic", "zhipu", "litellm", "volcengine", "minimax", "mimo", "deepseek", "agnes", or "ollama".
    #[serde(default)]
    pub driver: String,

    /// Available models for this source.
    #[serde(default)]
    pub models: Vec<ModelEntryConfig>,
}

/// A single model entry within a source
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct ModelEntryConfig {
    /// Model ID (used in API calls)
    pub id: String,

    /// Display name
    #[serde(default)]
    pub name: String,

    /// Supported input types (e.g. ["text", "image"])
    #[serde(default)]
    pub input: Vec<String>,

    /// Context window size in tokens
    #[serde(default, rename = "contextWindow")]
    pub context_window: usize,

    /// Maximum output tokens
    #[serde(default, rename = "max_tokens", alias = "maxTokens")]
    pub max_tokens: u32,

    /// Reasoning / thinking mode for this model.
    /// - `None` (default) — use CLI flag / global default
    /// - `Some(true)` — force enable thinking
    /// - `Some(false)` — force disable thinking
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,

    /// 固定推理档位（"off"/"low"/"medium"/"high"/"xhigh"/"max"）。
    /// - `None`（默认）— 沿用 `reasoning` bool 映射（true→Medium）
    /// - `Some(level)` — 视为开启思考并取该档位，优先级高于 `reasoning`
    ///
    /// 最终档位仍会被全局 max_reasoning 上限与各 provider 能力上限双重 clamp。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,

    /// API 风格：`"responses"` 使用 OpenAI Responses API（/v1/responses），
    /// 其他值或缺省使用 Chat Completions API（/v1/chat/completions）。
    /// gpt-5.6-sol 等模型只支持 Responses API。
    #[serde(default, rename = "apiStyle", skip_serializing_if = "Option::is_none")]
    pub api_style: Option<String>,
}

impl ModelEntryConfig {
    /// 获取模型的显示标签（name 非空且不等于 id 时显示 "name (id: id)"，否则只显示 id）
    pub fn display_label(&self) -> String {
        if self.name.is_empty() || self.name == self.id {
            self.id.clone()
        } else {
            format!("{} (id: {})", self.name, self.id)
        }
    }
}
