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
pub mod memory;
pub mod models;
pub mod paths;
pub mod permissions;
pub mod skills;
pub mod snapshot;
pub mod storage;
pub mod tools;
pub mod ui;
pub mod update;

// Re-exports for backward compatibility
pub use hooks::HooksConfig;
pub use legacy::{ApiConfig, ModelConfig};
pub use logging::LoggingConfig;
pub use memory::{MemoryConfig, ReflectionConfig};
pub use models::{ModelEntryConfig, ModelsConfig, ProviderModelsConfig};
pub use permissions::{PermissionConfig, PermissionModeConfig};
pub use skills::SkillsConfig;
pub use snapshot::{FileChange, FileChangeKind, FileSnapshot};
pub use storage::StorageConfig;
pub use tools::{AgentRoleConfig, AgentsConfig, ToolsConfig};
pub use ui::{TaskLifecycleConfig, TaskListConfig, UiConfig};
pub use update::UpdateConfig;

use serde::{Deserialize, Serialize};

/// Guidance 变更重载策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuidanceReloadPolicy {
    /// 每 turn 在 system prompt 前置 `[guidance 已更新] <diff head>`。
    Inject,
    /// 发 `<system-reminder>` 让 LLM 自行决定是否用 Read 重新读取。
    #[default]
    Remind,
    /// 发 system-reminder + TUI 状态栏标记，等用户确认后注入。
    Confirm,
}

/// Guidance 系统配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidanceConfig {
    /// guidance 文件变更时的重载策略。
    #[serde(default)]
    pub reload_policy: GuidanceReloadPolicy,
}

impl Default for GuidanceConfig {
    fn default() -> Self {
        Self {
            reload_policy: GuidanceReloadPolicy::Remind,
        }
    }
}

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
fn default_language() -> String {
    "en".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Guidance system configuration
    #[serde(default)]
    pub guidance: GuidanceConfig,

    /// Update check configuration
    #[serde(default)]
    pub update: UpdateConfig,

    /// Language preference for guidance files. Supported values: "en", "zh".
    /// Default: "en". Guidance files are loaded from `{language}/` subdirectory first,
    /// then fallback to root directory files.
    #[serde(default = "default_language")]
    pub language: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api: ApiConfig::default(),
            model: ModelConfig::default(),
            models: ModelsConfig::default(),
            tools: ToolsConfig::default(),
            agents: AgentsConfig::default(),
            ui: UiConfig::default(),
            permissions: PermissionConfig::default(),
            skills: SkillsConfig::default(),
            storage: StorageConfig::default(),
            hooks: HooksConfig::default(),
            memory: MemoryConfig::default(),
            logging: LoggingConfig::default(),
            guidance: GuidanceConfig::default(),
            update: UpdateConfig::default(),
            language: default_language(),
        }
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
        assert!(config.memory.enabled);
        assert_eq!(config.memory.max_entries, 100);
    }
}
