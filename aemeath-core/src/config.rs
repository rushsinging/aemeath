//! Configuration file management
//!
//! Supports layered configuration from multiple sources:
//! 1. Default values
//! 2. Global config file (~/.aemeath/config.json)
//! 3. Project config file (.aemeath/config.json)
//! 4. Environment variables
//! 5. Command line arguments

use crate::provider::Provider;
use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

/// Main configuration structure
///
/// ## Configuration layers (priority: high → low)
/// 1. Command line arguments
/// 2. Environment variables (`AEMEATH_*`)
/// 3. Project config file (`.aemeath/config.json`)
/// 4. Global config file (`~/.aemeath/config.json`)
/// 5. Built-in defaults
///
/// ## Legacy vs. new config
/// The `api` and `model` fields are **legacy** and only kept for backward
/// compatibility with older config files. New configurations should use the
/// `models` field with the `"provider/model_id"` format.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// **Legacy** — prefer `models` field. Still used by `/model`, `/config` commands.
    #[serde(default)]
    pub api: ApiConfig,

    /// **Legacy** — prefer `models` field. Still used by `/model`, `/config` commands.
    #[serde(default)]
    pub model: ModelConfig,

    /// Multi-provider model configuration (preferred).
    #[serde(default)]
    pub models: ModelsConfig,

    /// Tool configuration
    #[serde(default)]
    pub tools: ToolsConfig,

    /// UI configuration
    #[serde(default)]
    pub ui: UiConfig,

    /// Permission configuration
    #[serde(default)]
    pub permissions: PermissionConfig,

    /// Storage configuration
    #[serde(default)]
    pub storage: StorageConfig,
}

/// **Legacy** API configuration. Prefer using `models.providers` instead.
///
/// This is kept for backward compatibility with existing config files and
/// commands (`/model`, `/config`). New configurations should use `ModelsConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// LLM provider to use (None = unset, use base value; Some = explicitly set)
    #[serde(default)]
    pub provider: Option<Provider>,

    /// API key (can also be set via provider-specific env var)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// API base URL (default: provider-specific)
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

fn default_user_agent() -> String {
    format!("aemeath/{}", env!("CARGO_PKG_VERSION"))
}

fn default_timeout() -> u64 {
    300
}

fn default_retries() -> u32 {
    3
}

/// **Legacy** model configuration. Prefer using `models.providers[].models[]` instead.
///
/// This is kept for backward compatibility. New configurations should define
/// models under `models.providers.<name>.models` in config.json.
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

fn default_model() -> String {
    "claude-sonnet-4-6".to_string()
}

fn default_max_tokens() -> u32 {
    200000
}

fn default_context_size() -> usize {
    128000
}

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

    /// Find a model by "provider/model_id" string
    pub fn find_model(&self, query: &str) -> Option<(String, ProviderModelsConfig, ModelEntryConfig)> {
        if let Some((provider_name, model_query)) = query.split_once('/') {
            if let Some(provider_config) = self.providers.get(provider_name) {
                // Match by id first, then by name
                if let Some(model) = provider_config.models.iter().find(|m| m.id == model_query)
                    .or_else(|| provider_config.models.iter().find(|m| m.name == model_query))
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

    /// Whether this model supports reasoning/thinking
    #[serde(default)]
    pub reasoning: bool,

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

/// Tool configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    /// Enable/disable specific tools
    #[serde(default)]
    pub enabled: Vec<String>,

    /// Disabled tools
    #[serde(default)]
    pub disabled: Vec<String>,

    /// Tool-specific configurations
    #[serde(default)]
    pub settings: std::collections::HashMap<String, serde_json::Value>,
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Enable markdown rendering
    #[serde(default = "default_true")]
    pub markdown: bool,

    /// Enable syntax highlighting
    #[serde(default = "default_true")]
    pub syntax_highlight: bool,

    /// Show progress indicators
    #[serde(default = "default_true")]
    pub progress: bool,

    /// Color output
    #[serde(default = "default_true")]
    pub color: bool,

    /// Verbose output
    #[serde(default)]
    pub verbose: bool,

    /// TUI mode
    #[serde(default = "default_true")]
    pub tui: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            markdown: true,
            syntax_highlight: true,
            progress: true,
            color: true,
            verbose: false,
            tui: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Permission configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionConfig {
    /// Default permission mode
    #[serde(default)]
    pub mode: PermissionModeConfig,

    /// Auto-approved tools
    #[serde(default)]
    pub auto_approve: Vec<String>,

    /// Always-deny tools
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Permission mode configuration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionModeConfig {
    /// Ask for permission on every tool call
    #[default]
    Ask,
    /// Auto-approve read-only tools
    AutoRead,
    /// Auto-approve all tools (dangerous)
    AllowAll,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Directory for session storage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions_dir: Option<PathBuf>,

    /// Enable session persistence
    #[serde(default = "default_true")]
    pub persist_sessions: bool,

    /// Maximum sessions to keep
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,

    /// Enable history
    #[serde(default = "default_true")]
    pub history: bool,

    /// History file path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_file: Option<PathBuf>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            sessions_dir: None,
            persist_sessions: true,
            max_sessions: default_max_sessions(),
            history: true,
            history_file: None,
        }
    }
}

fn default_max_sessions() -> usize {
    100
}

/// Configuration manager
pub struct ConfigManager {
    /// Loaded configuration
    config: RwLock<Config>,
    /// Global config file path
    global_path: PathBuf,
    /// Project config file path
    project_path: Option<PathBuf>,
}

impl ConfigManager {
    /// Create a new config manager
    pub fn new(project_dir: Option<&Path>) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let global_path = home.join(".aemeath").join("config.json");
        let project_path = project_dir.map(|p| p.join(".aemeath").join("config.json"));

        Self {
            config: RwLock::new(Config::default()),
            global_path,
            project_path,
        }
    }

    /// Load configuration from all sources
    pub async fn load(&self) -> Result<Config, String> {
        let mut config = Config::default();

        // Load global config
        if self.global_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&self.global_path).await {
                if let Ok(global_config) = serde_json::from_str::<Config>(&content) {
                    config = Self::merge_config(config, global_config);
                }
            }
        }

        // Load project config
        if let Some(project_path) = &self.project_path {
            if project_path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(project_path).await {
                    if let Ok(project_config) = serde_json::from_str::<Config>(&content) {
                        config = Self::merge_config(config, project_config);
                    }
                }
            }
        }

        // Override with environment variables
        config = Self::apply_env_vars(config);

        *self.config.write().await = config.clone();
        Ok(config)
    }

    /// Apply environment variable overrides
    fn apply_env_vars(mut config: Config) -> Config {
        // Provider
        if let Ok(provider_str) = std::env::var("AEMEATH_PROVIDER") {
            if let Some(provider) = Provider::from_str(&provider_str) {
                config.api.provider = Some(provider);
            }
        }

        // API key - check provider-specific env var first
        let effective_provider = config.api.provider.unwrap_or_default();
        let provider_key_env = effective_provider.api_key_env();
        if let Ok(key) = std::env::var(provider_key_env) {
            config.api.key = Some(key);
        } else if let Ok(key) = std::env::var("LLM_API_KEY") {
            // Generic fallback (provider-agnostic)
            config.api.key = Some(key);
        }

        // Base URL - check provider-specific env var first
        let provider_base_env = effective_provider.base_url_env();
        if let Ok(url) = std::env::var("AEMEATH_BASE_URL") {
            config.api.base_url = Some(url);
        } else if let Ok(url) = std::env::var(provider_base_env) {
            config.api.base_url = Some(url);
        }

        // Model
        if let Ok(model) = std::env::var("AEMEATH_MODEL") {
            config.model.name = model;
        }

        // Max tokens
        if let Ok(max_tokens) = std::env::var("AEMEATH_MAX_TOKENS") {
            if let Ok(val) = max_tokens.parse() {
                config.model.max_tokens = val;
            }
        }

        // Context size
        if let Ok(context_size) = std::env::var("AEMEATH_CONTEXT_SIZE") {
            if let Ok(val) = context_size.parse() {
                config.model.context_size = val;
            }
        }

        // Permission mode
        if let Ok(mode) = std::env::var("AEMEATH_PERMISSION_MODE") {
            match mode.to_lowercase().as_str() {
                "ask" => config.permissions.mode = PermissionModeConfig::Ask,
                "auto_read" | "autoread" => config.permissions.mode = PermissionModeConfig::AutoRead,
                "allow_all" | "auto_all" | "autoall" => config.permissions.mode = PermissionModeConfig::AllowAll,
                _ => {}
            }
        }

        // Verbose
        if std::env::var("AEMEATH_VERBOSE").is_ok() {
            config.ui.verbose = true;
        }

        // No color
        if std::env::var("NO_COLOR").is_ok() {
            config.ui.color = false;
        }

        config
    }

    /// Merge two configs (overlay takes precedence)
    fn merge_config(base: Config, overlay: Config) -> Config {
        Config {
            api: ApiConfig {
                // None = unset, use base value; Some = explicitly set
                provider: overlay.api.provider.or(base.api.provider),
                key: overlay.api.key.or(base.api.key),
                base_url: overlay.api.base_url.or(base.api.base_url),
                user_agent: if overlay.api.user_agent != default_user_agent() {
                    overlay.api.user_agent
                } else {
                    base.api.user_agent
                },
                timeout: if overlay.api.timeout != default_timeout() {
                    overlay.api.timeout
                } else {
                    base.api.timeout
                },
                retries: if overlay.api.retries != default_retries() {
                    overlay.api.retries
                } else {
                    base.api.retries
                },
            },
            model: ModelConfig {
                name: if overlay.model.name != default_model() {
                    overlay.model.name
                } else {
                    base.model.name
                },
                max_tokens: if overlay.model.max_tokens != default_max_tokens() {
                    overlay.model.max_tokens
                } else {
                    base.model.max_tokens
                },
                context_size: if overlay.model.context_size != default_context_size() {
                    overlay.model.context_size
                } else {
                    base.model.context_size
                },
                temperature: overlay.model.temperature.or(base.model.temperature),
                top_k: overlay.model.top_k.or(base.model.top_k),
                top_p: overlay.model.top_p.or(base.model.top_p),
                stop_sequences: if !overlay.model.stop_sequences.is_empty() {
                    overlay.model.stop_sequences
                } else {
                    base.model.stop_sequences
                },
            },
            models: {
                // Merge providers from both configs
                let mut providers = base.models.providers;
                for (k, v) in overlay.models.providers {
                    providers.insert(k, v);
                }
                // Merge guidance from both configs
                let mut guidance = base.models.guidance;
                for (k, v) in overlay.models.guidance {
                    guidance.insert(k, v);
                }
                ModelsConfig {
                    mode: if overlay.models.mode.is_empty() { base.models.mode } else { overlay.models.mode },
                    default: if overlay.models.default.is_empty() { base.models.default } else { overlay.models.default },
                    providers,
                    guidance,
                }
            },
            tools: ToolsConfig {
                enabled: if !overlay.tools.enabled.is_empty() {
                    overlay.tools.enabled
                } else {
                    base.tools.enabled
                },
                disabled: if !overlay.tools.disabled.is_empty() {
                    overlay.tools.disabled
                } else {
                    base.tools.disabled
                },
                settings: Self::merge_maps(base.tools.settings, overlay.tools.settings),
            },
            ui: UiConfig {
                markdown: overlay.ui.markdown,
                syntax_highlight: overlay.ui.syntax_highlight,
                progress: overlay.ui.progress,
                color: overlay.ui.color,
                verbose: overlay.ui.verbose || base.ui.verbose,
                tui: overlay.ui.tui,
            },
            permissions: PermissionConfig {
                mode: if overlay.permissions.mode != PermissionModeConfig::default() {
                    overlay.permissions.mode
                } else {
                    base.permissions.mode
                },
                auto_approve: if !overlay.permissions.auto_approve.is_empty() {
                    overlay.permissions.auto_approve
                } else {
                    base.permissions.auto_approve
                },
                deny: if !overlay.permissions.deny.is_empty() {
                    overlay.permissions.deny
                } else {
                    base.permissions.deny
                },
            },
            storage: StorageConfig {
                sessions_dir: overlay.storage.sessions_dir.or(base.storage.sessions_dir),
                persist_sessions: overlay.storage.persist_sessions,
                max_sessions: overlay.storage.max_sessions,
                history: overlay.storage.history,
                history_file: overlay.storage.history_file.or(base.storage.history_file),
            },
        }
    }

    /// Merge two hashmaps
    fn merge_maps(
        base: std::collections::HashMap<String, serde_json::Value>,
        overlay: std::collections::HashMap<String, serde_json::Value>,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut result = base;
        result.extend(overlay);
        result
    }

    /// Get current config
    pub async fn get(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Save configuration to global file
    pub async fn save_global(&self) -> Result<(), String> {
        let config = self.config.read().await.clone();

        // Ensure parent directory exists
        if let Some(parent) = self.global_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;

        tokio::fs::write(&self.global_path, content)
            .await
            .map_err(|e| format!("Failed to write config: {e}"))?;

        Ok(())
    }

    /// Save configuration to project file
    pub async fn save_project(&self) -> Result<(), String> {
        let project_path = self
            .project_path
            .as_ref()
            .ok_or("No project directory set")?;

        let config = self.config.read().await.clone();

        // Ensure parent directory exists
        if let Some(parent) = project_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;

        tokio::fs::write(project_path, content)
            .await
            .map_err(|e| format!("Failed to write config: {e}"))?;

        Ok(())
    }

    /// Update configuration
    pub async fn update<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut Config),
    {
        let mut config = self.config.write().await;
        f(&mut config);
        drop(config);
        self.save_global().await
    }

    /// Get global config path
    pub fn global_path(&self) -> &Path {
        &self.global_path
    }

    /// Get project config path
    pub fn project_path(&self) -> Option<&Path> {
        self.project_path.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.model.name, "claude-sonnet-4-6");
        assert_eq!(config.model.max_tokens, 200000);
        assert!(config.ui.markdown);
        assert!(config.storage.persist_sessions);
    }

    #[test]
    fn test_config_manager_creation() {
        let mgr = ConfigManager::new(None);
        assert!(mgr.global_path().to_string_lossy().contains("aemeath"));
    }

    #[test]
    fn test_provider() {
        assert_eq!(Provider::from_str("anthropic"), Some(Provider::Anthropic));
        assert_eq!(Provider::from_str("openai"), Some(Provider::OpenAI));
        assert_eq!(Provider::from_str("deepseek"), Some(Provider::DeepSeek));
        assert_eq!(Provider::from_str("moonshot"), Some(Provider::Moonshot));
        assert_eq!(Provider::from_str("dashscope"), Some(Provider::DashScope));
        assert_eq!(Provider::from_str("invalid"), None);
    }

    #[test]
    fn test_provider_defaults() {
        assert_eq!(Provider::Anthropic.default_base_url(), "https://api.anthropic.com");
        assert_eq!(Provider::OpenAI.default_base_url(), "https://api.openai.com");
        assert_eq!(Provider::Anthropic.default_model(), "claude-sonnet-4-6");
        assert_eq!(Provider::OpenAI.default_model(), "gpt-4o");
    }
}