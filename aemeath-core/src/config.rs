//! Configuration file management
//!
//! Supports layered configuration from multiple sources:
//! 1. Default values
//! 2. Global config file (~/.config/aemeath/config.json)
//! 3. Project config file (.aemeath/config.json)
//! 4. Environment variables
//! 5. Command line arguments

use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// API configuration
    #[serde(default)]
    pub api: ApiConfig,

    /// Model configuration
    #[serde(default)]
    pub model: ModelConfig,

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

/// API configuration
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

/// Model configuration
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
    AutoAll,
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
        let global_path = home.join(".config").join("aemeath").join("config.json");
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
        } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            // Legacy support
            config.api.key = Some(key);
        } else if let Ok(key) = std::env::var("LLM_API_KEY") {
            // Generic fallback
            config.api.key = Some(key);
        }

        // Base URL
        if let Ok(url) = std::env::var("AEMEATH_BASE_URL") {
            config.api.base_url = Some(url);
        } else if let Ok(url) = std::env::var("ANTHROPIC_BASE_URL") {
            // Legacy support
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
                "auto_all" | "autoall" => config.permissions.mode = PermissionModeConfig::AutoAll,
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