//! 多来源模型配置

use crate::provider::ApiDriverKind;
use serde::{Deserialize, Serialize};
use std::fmt;

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
    pub providers: std::collections::HashMap<String, ProviderModelsConfig>,

    /// Guidance file overrides, keyed by glob pattern (e.g. "zhipu/*" → "~/.aemeath/guidance/glm.md").
    #[serde(default)]
    pub guidance: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    pub source_key: String,
    pub source_config: ProviderModelsConfig,
    pub model: ModelEntryConfig,
    pub api: ApiDriverKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelResolveError {
    MissingSelection {
        available_sources: Vec<String>,
    },
    InvalidFormat {
        selection: String,
    },
    UnknownSource {
        source: String,
        available_sources: Vec<String>,
    },
    UnknownModel {
        source: String,
        query: String,
        available_models: Vec<String>,
    },
    UnknownApi {
        source: String,
        api: String,
    },
}

impl fmt::Display for ModelResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSelection { available_sources } => write!(
                f,
                "未指定模型。请使用 --model <来源>/<模型>。可用来源：\n  {}",
                available_sources.join("\n  ")
            ),
            Self::InvalidFormat { selection } => {
                write!(f, "模型选择 '{}' 格式无效，请使用 <来源>/<模型>", selection)
            }
            Self::UnknownSource {
                source,
                available_sources,
            } => write!(
                f,
                "未找到模型来源 '{}'。\n可用来源：\n  {}",
                source,
                available_sources.join("\n  ")
            ),
            Self::UnknownModel {
                source,
                query,
                available_models,
            } => write!(
                f,
                "来源 '{}' 中未找到模型 '{}'。\n可用模型：\n  {}",
                source,
                query,
                available_models.join("\n  ")
            ),
            Self::UnknownApi { source, api } => write!(
                f,
                "来源 '{}' 的 api '{}' 不受支持。支持的 api：anthropic, openai, zhipu, litellm",
                source, api
            ),
        }
    }
}

impl std::error::Error for ModelResolveError {}

impl ModelsConfig {
    pub fn resolve_model_selection(
        &self,
        selection: &str,
    ) -> Result<ResolvedModel, ModelResolveError> {
        let (source_query, model_query) =
            selection
                .split_once('/')
                .ok_or_else(|| ModelResolveError::InvalidFormat {
                    selection: selection.to_string(),
                })?;
        if source_query.is_empty() || model_query.is_empty() {
            return Err(ModelResolveError::InvalidFormat {
                selection: selection.to_string(),
            });
        }

        let available_sources = self.available_source_keys();
        let (source_key, source_config) = self
            .providers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(source_query))
            .ok_or_else(|| ModelResolveError::UnknownSource {
                source: source_query.to_string(),
                available_sources: available_sources.clone(),
            })?;

        let model = source_config
            .models
            .iter()
            .find(|m| m.name == model_query)
            .or_else(|| source_config.models.iter().find(|m| m.id == model_query))
            .or_else(|| {
                let norm = normalize_model_key(model_query);
                source_config
                    .models
                    .iter()
                    .find(|m| normalize_model_key(&m.name) == norm)
            })
            .or_else(|| {
                let norm = normalize_model_key(model_query);
                source_config
                    .models
                    .iter()
                    .find(|m| normalize_model_key(&m.id) == norm)
            })
            .cloned()
            .ok_or_else(|| ModelResolveError::UnknownModel {
                source: source_key.clone(),
                query: model_query.to_string(),
                available_models: source_config
                    .models
                    .iter()
                    .map(|m| {
                        if m.name.is_empty() || m.name == m.id {
                            m.id.clone()
                        } else {
                            format!("{} (id: {})", m.name, m.id)
                        }
                    })
                    .collect(),
            })?;

        let api = ApiDriverKind::from_str(source_config.api.as_str()).ok_or_else(|| {
            ModelResolveError::UnknownApi {
                source: source_key.clone(),
                api: source_config.api.clone(),
            }
        })?;

        Ok(ResolvedModel {
            source_key: source_key.clone(),
            source_config: source_config.clone(),
            model,
            api,
        })
    }

    pub fn resolve_default_model(&self) -> Result<ResolvedModel, ModelResolveError> {
        if !self.default.is_empty() {
            return self.resolve_model_selection(&self.default);
        }

        let mut candidates = self
            .providers
            .iter()
            .filter_map(|(source_key, source_config)| {
                source_config
                    .models
                    .first()
                    .map(|model| (source_key, source_config, model))
            });
        let first = candidates.next();
        if let Some((source_key, source_config, model)) = first {
            if candidates.next().is_none() && source_config.models.len() == 1 {
                let api = ApiDriverKind::from_str(source_config.api.as_str()).ok_or_else(|| {
                    ModelResolveError::UnknownApi {
                        source: source_key.clone(),
                        api: source_config.api.clone(),
                    }
                })?;
                return Ok(ResolvedModel {
                    source_key: source_key.clone(),
                    source_config: source_config.clone(),
                    model: model.clone(),
                    api,
                });
            }
        }

        Err(ModelResolveError::MissingSelection {
            available_sources: self.available_source_keys(),
        })
    }

    pub fn available_source_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.providers.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// List all available models as (source_key, model_entry) pairs.
    pub fn list_models(&self) -> Vec<(String, ModelEntryConfig)> {
        let mut result = Vec::new();
        for (source_key, source_config) in &self.providers {
            for model in &source_config.models {
                result.push((source_key.clone(), model.clone()));
            }
        }
        result.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
        result
    }

    /// Find a model by "<source>/<model>" selection string. Matches in order:
    ///  1. exact `name`
    ///  2. exact `id`
    ///  3. normalized `name` (case-insensitive, decorative chars stripped —
    ///     e.g. `DeepSeek-V4-Pro` matches `DeepSeek-V4-Pro ⚡`)
    ///  4. normalized `id`
    ///
    /// Normalization keeps only alphanumerics, `-`, `_`, `.` and lowercases,
    /// so trailing ⚡/emoji decoration in display names doesn't require the
    /// user to type the exact symbol when setting `"default"`.
    pub fn find_model(
        &self,
        query: &str,
    ) -> Option<(String, ProviderModelsConfig, ModelEntryConfig)> {
        if let Some((source_query, model_query)) = query.split_once('/') {
            if let Some((actual_source_key, source_config)) = self
                .providers
                .iter()
                .find(|(name, _)| name.to_lowercase() == source_query.to_lowercase())
            {
                if let Some(model) = source_config
                    .models
                    .iter()
                    .find(|m| m.name == model_query)
                    .or_else(|| source_config.models.iter().find(|m| m.id == model_query))
                {
                    return Some((
                        actual_source_key.to_string(),
                        source_config.clone(),
                        model.clone(),
                    ));
                }
                // Fuzzy fallback
                let norm = normalize_model_key(model_query);
                if let Some(model) = source_config
                    .models
                    .iter()
                    .find(|m| normalize_model_key(&m.name) == norm)
                    .or_else(|| {
                        source_config
                            .models
                            .iter()
                            .find(|m| normalize_model_key(&m.id) == norm)
                    })
                {
                    return Some((
                        actual_source_key.to_string(),
                        source_config.clone(),
                        model.clone(),
                    ));
                }
            }
        }
        None
    }

    /// Look up a source entry case-insensitively. Guards against a
    /// silent fallback bug where a lowercased lookup misses a config
    /// key spelled "Zhipu" (capital) and callers pick stale/unrelated
    /// source credentials from a HashMap iteration instead.
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

/// Valid reasoning_effort values.
const VALID_REASONING_EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];

/// Validate a reasoning_effort value. Returns `Ok(())` if valid.
pub fn validate_reasoning_effort(effort: &str) -> Result<(), String> {
    if VALID_REASONING_EFFORTS.contains(&effort) {
        Ok(())
    } else {
        Err(format!(
            "Invalid reasoning_effort '{}'. Valid values: {}",
            effort,
            VALID_REASONING_EFFORTS.join(", ")
        ))
    }
}

/// Check whether a model id supports reasoning_effort (OpenAI GPT-5.x / o-series).
pub fn supports_reasoning_effort(model_id: &str) -> bool {
    let lower = model_id.to_lowercase();
    lower.starts_with("gpt-5")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
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

    /// API driver: "openai", "anthropic", "zhipu", or "litellm".
    #[serde(default)]
    pub api: String,

    /// Available models for this source.
    #[serde(default)]
    pub models: Vec<ModelEntryConfig>,
}

/// A single model entry within a source
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config() -> ModelsConfig {
        let mut providers = HashMap::new();
        providers.insert(
            "LiteLLM".to_string(),
            ProviderModelsConfig {
                base_url: "http://localhost:4000".to_string(),
                api_key: String::new(),
                api: "openai".to_string(),
                models: vec![ModelEntryConfig {
                    id: "gpt-5.5".to_string(),
                    name: "GPT-5.5".to_string(),
                    input: vec!["text".to_string()],
                    context_window: 200_000,
                    max_tokens: 32_000,
                    reasoning: Some(false),
                    reasoning_effort: None,
                }],
            },
        );
        ModelsConfig {
            mode: String::new(),
            default: String::new(),
            providers,
            guidance: HashMap::new(),
        }
    }

    #[test]
    fn test_find_model_exact_source_case_insensitive() {
        let config = test_config();
        let result = config.find_model("litellm/gpt-5.5");
        assert!(result.is_some());
        let (source, _, model) = result.unwrap();
        assert_eq!(source, "LiteLLM");
        assert_eq!(model.id, "gpt-5.5");
        assert_eq!(model.reasoning, Some(false));
    }

    #[test]
    fn test_find_model_display_name_case_insensitive_source() {
        let config = test_config();
        let result = config.find_model("litellm/GPT-5.5");
        assert!(result.is_some());
        let (_, _, model) = result.unwrap();
        assert_eq!(model.name, "GPT-5.5");
    }

    #[test]
    fn test_find_model_unknown_source_returns_none() {
        let config = test_config();
        let result = config.find_model("openai/gpt-5.5");
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_reasoning_effort_valid() {
        for valid in &["none", "low", "medium", "high", "xhigh"] {
            assert!(validate_reasoning_effort(valid).is_ok());
        }
    }

    #[test]
    fn test_validate_reasoning_effort_invalid() {
        assert!(validate_reasoning_effort("turbo").is_err());
        assert!(validate_reasoning_effort("HIGH").is_err());
        assert!(validate_reasoning_effort("").is_err());
    }

    #[test]
    fn test_supports_reasoning_effort() {
        assert!(supports_reasoning_effort("gpt-5.5"));
        assert!(supports_reasoning_effort("gpt-5"));
        assert!(supports_reasoning_effort("o1"));
        assert!(supports_reasoning_effort("o3-mini"));
        assert!(supports_reasoning_effort("o4-mini"));
        assert!(!supports_reasoning_effort("gpt-4o"));
        assert!(!supports_reasoning_effort("deepseek-r1"));
        assert!(!supports_reasoning_effort("claude-opus-4"));
    }

    #[test]
    fn test_model_entry_reasoning_effort_deserialize() {
        let json = r#"{"id":"gpt-5.5","reasoning_effort":"low"}"#;
        let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(entry.reasoning_effort, Some("low".to_string()));
        assert_eq!(entry.id, "gpt-5.5");
    }

    #[test]
    fn test_model_entry_reasoning_effort_default_none() {
        let json = r#"{"id":"gpt-4o"}"#;
        let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
        assert!(entry.reasoning_effort.is_none());
    }

    fn resolver_config() -> ModelsConfig {
        let mut providers = HashMap::new();
        providers.insert(
            "Zhipu".to_string(),
            ProviderModelsConfig {
                base_url: "https://zhipu.example.com".to_string(),
                api_key: "zhipu-key".to_string(),
                api: "zhipu".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    name: "GLM 5.1".to_string(),
                    context_window: 128_000,
                    max_tokens: 32_000,
                    reasoning: Some(true),
                    ..Default::default()
                }],
            },
        );
        providers.insert(
            "LiteLLM".to_string(),
            ProviderModelsConfig {
                base_url: "https://litellm.example.com".to_string(),
                api_key: "litellm-key".to_string(),
                api: "litellm".to_string(),
                models: vec![ModelEntryConfig {
                    id: "anthropic/claude-opus-4-7".to_string(),
                    name: "Claude via LiteLLM".to_string(),
                    context_window: 200_000,
                    max_tokens: 16_000,
                    reasoning: None,
                    ..Default::default()
                }],
            },
        );
        ModelsConfig {
            mode: String::new(),
            default: "Zhipu/glm-5.1".to_string(),
            providers,
            guidance: HashMap::new(),
        }
    }

    #[test]
    fn test_resolve_model_selection_zhipu() {
        let config = resolver_config();
        let resolved = config.resolve_model_selection("zhipu/glm-5.1").unwrap();
        assert_eq!(resolved.source_key, "Zhipu");
        assert_eq!(resolved.model.id, "glm-5.1");
        assert_eq!(resolved.api, crate::provider::ApiDriverKind::Zhipu);
        assert_eq!(resolved.source_config.api, "zhipu");
    }

    #[test]
    fn test_resolve_model_selection_litellm_model_id_with_slash() {
        let config = resolver_config();
        let resolved = config
            .resolve_model_selection("LiteLLM/anthropic/claude-opus-4-7")
            .unwrap();
        assert_eq!(resolved.source_key, "LiteLLM");
        assert_eq!(resolved.model.id, "anthropic/claude-opus-4-7");
        assert_eq!(resolved.api, crate::provider::ApiDriverKind::LiteLLM);
    }

    #[test]
    fn test_resolve_model_selection_unknown_source_lists_available() {
        let config = resolver_config();
        let err = config
            .resolve_model_selection("Missing/glm-5.1")
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("未找到模型来源 'Missing'"));
        assert!(message.contains("Zhipu"));
        assert!(message.contains("LiteLLM"));
    }

    #[test]
    fn test_resolve_model_selection_unknown_model_lists_available() {
        let config = resolver_config();
        let err = config.resolve_model_selection("Zhipu/glm-x").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("来源 'Zhipu' 中未找到模型 'glm-x'"));
        assert!(message.contains("glm-5.1"));
    }

    #[test]
    fn test_resolve_model_selection_rejects_openai_compatible_api() {
        let mut config = resolver_config();
        let source = config.providers.get_mut("Zhipu").unwrap();
        source.api = "openai-compatible".to_string();

        let err = config.resolve_model_selection("Zhipu/glm-5.1").unwrap_err();

        assert_eq!(
            err,
            ModelResolveError::UnknownApi {
                source: "Zhipu".to_string(),
                api: "openai-compatible".to_string(),
            }
        );
    }

    #[test]
    fn test_resolve_default_model_uses_config_default() {
        let config = resolver_config();
        let resolved = config.resolve_default_model().unwrap();
        assert_eq!(resolved.source_key, "Zhipu");
        assert_eq!(resolved.model.id, "glm-5.1");
    }
}
