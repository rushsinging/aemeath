//! Configuration file management
//!
//! Supports layered configuration from multiple sources:
//! 1. Default values
//! 2. Global config file (~/.aemeath/config.json)
//! 3. Project config file (.aemeath/config.json)
//! 4. Environment variables
//! 5. Command line arguments

pub mod legacy;
pub mod manager;
pub mod models;
pub mod permissions;
pub mod skills;
pub mod storage;
pub mod tools;
pub mod ui;

// Re-exports for backward compatibility
pub use legacy::{ApiConfig, ModelConfig};
pub use manager::ConfigManager;
pub use models::{ModelEntryConfig, ModelsConfig, ProviderModelsConfig};
pub use permissions::{PermissionConfig, PermissionModeConfig};
pub use skills::SkillsConfig;
pub use storage::StorageConfig;
pub use tools::{AgentRoleConfig, AgentsConfig, ToolsConfig};
pub use ui::UiConfig;

use serde::{Deserialize, Serialize};

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

    /// Agent configuration
    #[serde(default)]
    pub agents: AgentsConfig,

    /// UI configuration
    #[serde(default)]
    pub ui: UiConfig,

    /// Permission configuration
    #[serde(default)]
    pub permissions: PermissionConfig,

    /// Skill configuration
    #[serde(default)]
    pub skills: SkillsConfig,

    /// Storage configuration
    #[serde(default)]
    pub storage: StorageConfig,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;

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
