//! 模型解析与查找逻辑

use crate::config::models::error::ModelResolveError;
use crate::config::models::types::*;
use crate::provider::ApiDriverKind;

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
        let (source_key, source_config) =
            self.provider_entry_ci(source_query).ok_or_else(|| {
                ModelResolveError::UnknownSource {
                    source: source_query.to_string(),
                    available_sources: available_sources.clone(),
                }
            })?;

        let model = find_model_in_provider(source_config, model_query)
            .cloned()
            .ok_or_else(|| ModelResolveError::UnknownModel {
                source: source_key.clone(),
                query: model_query.to_string(),
                available_models: available_model_labels(source_config),
            })?;

        let api = resolve_api_driver(&source_key, source_config)?;

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
                let api = resolve_api_driver(source_key, source_config)?;
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
        let (source_query, model_query) = query.split_once('/')?;
        let (source_key, source_config) = self.provider_entry_ci(source_query)?;
        let source_key = source_key.clone();
        let source_config = source_config.clone();
        find_model_in_provider(&source_config, model_query)
            .cloned()
            .map(|model| (source_key, source_config, model))
    }

    /// Look up a source entry case-insensitively.
    pub fn provider_ci(&self, name: &str) -> Option<&ProviderModelsConfig> {
        self.provider_entry_ci(name).map(|(_, v)| v)
    }

    /// 内部方法：大小写不敏感查找 provider 条目
    fn provider_entry_ci(&self, name: &str) -> Option<(&String, &ProviderModelsConfig)> {
        let lc = name.to_lowercase();
        self.providers.iter().find(|(k, _)| k.to_lowercase() == lc)
    }
}

// === 共享辅助函数 ===

/// 在指定 provider 中按名称/ID 查找模型（含模糊匹配）
fn find_model_in_provider<'a>(
    source_config: &'a ProviderModelsConfig,
    model_query: &str,
) -> Option<&'a ModelEntryConfig> {
    source_config
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
                .or_else(|| {
                    source_config
                        .models
                        .iter()
                        .find(|m| normalize_model_key(&m.id) == norm)
                })
        })
}

/// 生成可用模型标签列表（用于错误提示）
fn available_model_labels(source_config: &ProviderModelsConfig) -> Vec<String> {
    source_config
        .models
        .iter()
        .map(|m| m.display_label())
        .collect()
}

/// 解析 API driver 类型
fn resolve_api_driver(
    source_key: &str,
    source_config: &ProviderModelsConfig,
) -> Result<ApiDriverKind, ModelResolveError> {
    ApiDriverKind::from_str(source_config.api.as_str()).ok_or_else(|| {
        ModelResolveError::UnknownApi {
            source: source_key.to_string(),
            api: source_config.api.clone(),
        }
    })
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
