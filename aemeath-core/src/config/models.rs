//! 多供应商模型配置

use serde::{Deserialize, Serialize};

/// Multi-provider model configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsConfig {
    /// Merge mode: "merge" to combine with env/CLI settings
    #[serde(default)]
    pub mode: String,

    /// Default provider and model in "provider/model_id" format (e.g. "zhipu/glm-5.1")
    /// Used when no --provider or AEMEATH_PROVIDER is set
    #[serde(default)]
    pub default: String,

    /// Provider configurations keyed by provider name
    #[serde(default)]
    pub providers: std::collections::HashMap<String, ProviderModelsConfig>,

    /// Guidance file overrides, keyed by glob pattern (e.g. "zhipu/*" → "~/.aemeath/guidance/glm.md")
    #[serde(default)]
    pub guidance: std::collections::HashMap<String, String>,
}

impl ModelsConfig {
    /// List all available models as (provider_name, model_entry) pairs
    pub fn list_models(&self) -> Vec<(String, ModelEntryConfig)> {
        let mut result = Vec::new();
        for (provider_name, provider_config) in &self.providers {
            for model in &provider_config.models {
                result.push((provider_name.clone(), model.clone()));
            }
        }
        result.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        result
    }

    /// Find a model by "provider/model_query" string. Matches in order:
    ///  1. exact `name`
    ///  2. exact `id`
    ///  3. normalized `name` (case-insensitive, decorative chars stripped —
    ///     e.g. `DeepSeek-V4-Pro` matches `DeepSeek-V4-Pro ⚡`)
    ///  4. normalized `id`
    ///
    /// Normalization keeps only alphanumerics, `-`, `_`, `.` and lowercases,
    /// so trailing ⚡/emoji decoration in display names doesn't require the
    /// user to type the exact symbol when setting `"default"`.
    pub fn find_model(&self, query: &str) -> Option<(String, ProviderModelsConfig, ModelEntryConfig)> {
        if let Some((provider_name, model_query)) = query.split_once('/') {
            if let Some(provider_config) = self.providers.get(provider_name) {
                if let Some(model) = provider_config.models.iter().find(|m| m.name == model_query)
                    .or_else(|| provider_config.models.iter().find(|m| m.id == model_query))
                {
                    return Some((
                        provider_name.to_string(),
                        provider_config.clone(),
                        model.clone(),
                    ));
                }
                // Fuzzy fallback
                let norm = normalize_model_key(model_query);
                if let Some(model) = provider_config.models.iter()
                    .find(|m| normalize_model_key(&m.name) == norm)
                    .or_else(|| provider_config.models.iter()
                        .find(|m| normalize_model_key(&m.id) == norm))
                {
                    return Some((
                        provider_name.to_string(),
                        provider_config.clone(),
                        model.clone(),
                    ));
                }
            }
        }
        None
    }

    /// Look up a provider entry case-insensitively. Guards against a
    /// silent fallback bug where a lowercased lookup misses a config
    /// key spelled "Zhipu" (capital) and callers pick a stale/unrelated
    /// provider's credentials from a HashMap iteration instead.
    pub fn provider_ci(&self, name: &str) -> Option<&ProviderModelsConfig> {
        let lc = name.to_lowercase();
        self.providers
            .iter()
            .find(|(k, _)| k.to_lowercase() == lc)
            .map(|(_, v)| v)
    }
}

/// Normalize a model key for fuzzy matching — keep alphanumerics and
/// `-_.`, drop spaces / emoji / decoration, lowercase. Lets
/// `"DeepSeek-V4-Pro"` match `"DeepSeek-V4-Pro ⚡"`.
pub(crate) fn normalize_model_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Configuration for a single provider within models config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderModelsConfig {
    /// Base URL for the provider API
    #[serde(default, rename = "baseUrl")]
    pub base_url: String,

    /// API key for this provider
    #[serde(default, rename = "apiKey")]
    pub api_key: String,

    /// API type: "openai-completions" or "anthropic"
    #[serde(default)]
    pub api: String,

    /// Available models for this provider
    #[serde(default)]
    pub models: Vec<ModelEntryConfig>,
}

/// A single model entry within a provider
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    #[serde(default, rename = "maxTokens")]
    pub max_tokens: u32,
}
