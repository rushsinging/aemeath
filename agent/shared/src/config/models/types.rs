//! 多来源模型配置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const VOLCENGINE_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/coding/v3";

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
    pub api: String,
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

    /// API driver: "openai", "anthropic", "zhipu", "litellm", or "volcengine".
    #[serde(default)]
    pub api: String,

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

    /// Maximum thinking tokens
    #[serde(default, rename = "thinking_max_tokens", alias = "thinkingMaxTokens")]
    pub thinking_max_tokens: u32,

    /// Reasoning / thinking mode for this model.
    /// - `None` (default) — use CLI flag / global default
    /// - `Some(true)` — force enable thinking
    /// - `Some(false)` — force disable thinking
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,

    /// Reasoning effort level (only effective for models that support it,
    /// e.g. OpenAI GPT-5.x / o-series).
    /// - `None` (default) — use source default (usually "medium")
    /// - Valid values: `"none"`, `"low"`, `"medium"`, `"high"`, `"xhigh"`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
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

/// Built-in Volcengine Coding Plan model source.
pub fn volcengine_coding_plan_config() -> ModelsConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "Volcengine".to_string(),
        ProviderModelsConfig {
            base_url: VOLCENGINE_BASE_URL.to_string(),
            api_key: String::new(),
            api: "volcengine".to_string(),
            models: default_volcengine_models(),
        },
    );
    ModelsConfig {
        mode: "merge".to_string(),
        default: "Volcengine/doubao-seed-2-0-code-preview-260215".to_string(),
        providers,
        guidance: HashMap::new(),
    }
}

fn volcengine_model(
    id: &str,
    name: &str,
    context_window: usize,
    max_tokens: u32,
    thinking_max_tokens: u32,
) -> ModelEntryConfig {
    ModelEntryConfig {
        id: id.to_string(),
        name: name.to_string(),
        input: vec!["text".to_string(), "image".to_string(), "video".to_string()],
        context_window,
        max_tokens,
        thinking_max_tokens,
        reasoning: Some(true),
        reasoning_effort: None,
    }
}

fn default_volcengine_models() -> Vec<ModelEntryConfig> {
    vec![
        volcengine_model(
            "doubao-seed-2-0-code-preview-260215",
            "doubao-seed-2-0-code",
            262_144,
            131_072,
            131_072,
        ),
        volcengine_model(
            "doubao-seed-2-0-pro-260215",
            "doubao-seed-2-0-pro",
            262_144,
            131_072,
            131_072,
        ),
        volcengine_model(
            "doubao-seed-2-0-lite-260428",
            "doubao-seed-2-0-lite",
            262_144,
            131_072,
            131_072,
        ),
        volcengine_model(
            "doubao-seed-2-0-mini-260428",
            "doubao-seed-2-0-mini",
            262_144,
            131_072,
            131_072,
        ),
        volcengine_model("glm-4-7-251222", "glm-4-7", 204_800, 131_072, 131_072),
        volcengine_model(
            "deepseek-v3-2-251201",
            "deepseek-v3-2",
            131_072,
            32_768,
            32_768,
        ),
        volcengine_model(
            "kimi-k2-thinking-251104",
            "kimi-k2",
            262_144,
            32_768,
            32_768,
        ),
    ]
}
