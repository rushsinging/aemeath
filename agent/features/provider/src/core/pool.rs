//! LLM Client Pool — cache and reuse `LlmClient` instances keyed by `"provider/model_id"`.
//!
//! When a sub-agent requests a specific model, the pool either returns a
//! cached client or creates one on-the-fly from `ModelsConfig`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::api::ProviderDriverKind;
use crate::LOG_TARGET;
use share::config::ModelsConfig;

use crate::core::client::{LlmClient, OpenAIProviderConfig};

/// A pool of `LlmClient` instances keyed by model spec (`"provider/model_id"`).
///
/// Thread-safe: uses `tokio::sync::Mutex` for the inner map so async
/// client creation does not block other lookups.
pub struct LlmClientPool {
    clients: tokio::sync::Mutex<HashMap<String, Arc<LlmClient>>>,
    default_client: Arc<LlmClient>,
    models_config: Arc<ModelsConfig>,
}

impl LlmClientPool {
    /// Create a new pool.
    ///
    /// * `default_client` — the client used when no model spec is provided.
    /// * `models_config`   — used to resolve `"provider/model_id"` strings and
    ///   build new clients dynamically.
    pub fn new(default_client: Arc<LlmClient>, models_config: Arc<ModelsConfig>) -> Self {
        Self {
            clients: tokio::sync::Mutex::new(HashMap::new()),
            default_client,
            models_config,
        }
    }

    /// Get a client for the given model spec.
    ///
    /// * `model_spec = None` → returns the default client.
    /// * `model_spec = Some("provider/model_id")` → returns a cached client
    ///   or creates one from `ModelsConfig`.
    ///
    /// If the model spec cannot be resolved, falls back to the default client
    /// and logs a warning.
    pub async fn get_client(&self, model_spec: Option<&str>) -> Arc<LlmClient> {
        let Some(spec) = model_spec else {
            return self.default_client.clone();
        };

        // Fast path: already cached
        {
            let clients = self.clients.lock().await;
            if let Some(client) = clients.get(spec) {
                return client.clone();
            }
        }

        // Slow path: create a new client
        match self.create_client(spec) {
            Ok(client) => {
                let client = Arc::new(client);
                self.clients
                    .lock()
                    .await
                    .insert(spec.to_string(), client.clone());
                log::info!(target: LOG_TARGET, "[LlmClientPool] created new client for {:?}", spec);
                client
            }
            Err(e) => {
                log::warn!(target: LOG_TARGET,
                    "[LlmClientPool] failed to create client for {:?}: {}. Falling back to default.",
                    spec,
                    e
                );
                self.default_client.clone()
            }
        }
    }

    /// Resolve a `"provider/model_id"` spec and create an `LlmClient`.
    fn create_client(&self, spec: &str) -> Result<LlmClient, String> {
        let (provider_name, model_query) = spec.split_once('/').ok_or_else(|| {
            format!(
                "invalid model spec '{}', expected 'provider/model_id'",
                spec
            )
        })?;

        // Look up in ModelsConfig
        let (provider_config, model_entry) = self
            .models_config
            .find_model(spec)
            .map(|(_pn, pc, me)| (pc, me))
            .ok_or_else(|| {
                let available: Vec<String> = self
                    .models_config
                    .providers
                    .get(provider_name)
                    .map(|p| {
                        p.models
                            .iter()
                            .map(|m| format!("{} (id: {})", m.name, m.id))
                            .collect()
                    })
                    .unwrap_or_default();
                format!(
                    "model '{}' not found under provider '{}'. Available: {}",
                    model_query,
                    provider_name,
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                )
            })?;

        // Resolve ProviderDriverKind from config (the `driver` field)
        let driver = ProviderDriverKind::parse(&provider_config.driver)
            .unwrap_or(ProviderDriverKind::OpenAI);

        // Build OpenAI provider config for OpenAI-compatible providers.
        // Anthropic 与 Ollama 各有专用 provider，不生成 openai_config。
        let openai_config = if !matches!(
            driver,
            ProviderDriverKind::Anthropic | ProviderDriverKind::Ollama
        ) {
            Some(OpenAIProviderConfig::from_driver(driver, provider_name))
        } else {
            None
        };
        // API key — 由 ConfigAppService::load() 的 resolve_provider_api_keys
        // 在 config 加载时从 env 注入，pool 只读 provider_config.api_key。
        let api_key = if provider_config.api_key.is_empty() {
            return Err(format!(
                "no API key for provider '{}'. Set it in config.json or provider-specific env var",
                provider_name
            ));
        } else {
            provider_config.api_key.clone()
        };
        let base_url = if provider_config.base_url.is_empty() {
            None
        } else {
            Some(provider_config.base_url.clone())
        };

        let max_tokens = model_entry.max_tokens;
        if max_tokens == 0 {
            log::warn!(target: LOG_TARGET,
                "[LlmClientPool] max_tokens is 0 for '{}' — using 8192 default",
                spec
            );
        }
        let max_tokens = if max_tokens > 0 { max_tokens } else { 8192 };

        let reasoning = true; // reasoning is now a runtime toggle, always start enabled

        Ok(LlmClient::from_config(
            crate::core::client::LlmConfigOptions {
                driver,
                api_key,
                base_url,
                model: model_entry.id,
                max_tokens,
                reasoning,
                reasoning_config: None,
                openai_config,
            },
        ))
    }

    /// Get the default client.
    pub fn default_client(&self) -> Arc<LlmClient> {
        self.default_client.clone()
    }
}
