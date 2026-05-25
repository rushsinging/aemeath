//! Configuration file management
//!
//! Supports layered configuration from multiple sources:
//! 1. Default values
//! 2. Global config file (`~/.agents/aemeath.json` by default)
//! 3. Project config file (`{cwd}/.agents/aemeath.json`)
//! 4. Environment variables
//! 5. Command line arguments

pub mod hooks;
pub mod legacy;
pub mod logging;
pub mod manager;
pub mod memory;
pub mod models;
pub mod paths;
pub mod permissions;
pub mod skills;
pub mod storage;
pub mod tools;
pub mod ui;

// Re-exports for backward compatibility
pub use hooks::HooksConfig;
pub use legacy::{ApiConfig, ModelConfig};
pub use logging::LoggingConfig;
pub use manager::ConfigManager;
pub use memory::{MemoryConfig, ReflectionConfig};
pub use models::{
    volcengine_coding_plan_config, ModelEntryConfig, ModelsConfig, ProviderModelsConfig,
};
pub use permissions::{PermissionConfig, PermissionModeConfig};
pub use skills::SkillsConfig;
pub use storage::StorageConfig;
pub use tools::{AgentRoleConfig, AgentsConfig, ToolsConfig};
pub use ui::{TaskLifecycleConfig, TaskListConfig, UiConfig};

use serde::{Deserialize, Serialize};

/// Main configuration structure
///
/// ## Configuration layers (priority: high → low)
/// 1. Command line arguments
/// 2. Environment variables (`AEMEATH_*`)
/// 3. Project config file (`{cwd}/.agents/aemeath.json`)
/// 4. Global config file (`~/.agents/aemeath.json` by default)
/// 5. Built-in defaults
///
/// ## Legacy vs. new config
/// The `api` and `model` fields are **legacy** and only kept for backward
/// compatibility with older config files. New configurations should use the
/// `models` field with the `"<source>/<model>"` selection format.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// **Legacy** — prefer `models` field. Still used by `/model`, `/config` commands.
    #[serde(default)]
    pub api: ApiConfig,

    /// **Legacy** — prefer `models` field. Still used by `/model`, `/config` commands.
    #[serde(default)]
    pub model: ModelConfig,

    /// Multi-source model configuration (preferred).
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

    /// Hook configuration
    #[serde(default)]
    pub hooks: HooksConfig,

    /// Memory and reflection configuration
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
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
        assert!(config.memory.enabled);
        assert_eq!(config.memory.max_entries, 100);
    }

    #[test]
    fn test_config_manager_creation() {
        let _guard = super::paths::TEST_ENV_LOCK.lock().unwrap();
        let _ = std::env::remove_var(super::paths::AGENTS_DIR_ENV);
        let mgr = ConfigManager::new(None);
        assert!(mgr.global_path().to_string_lossy().contains(".agents"));
        assert!(mgr
            .global_path()
            .to_string_lossy()
            .ends_with("aemeath.json"));
    }
}
