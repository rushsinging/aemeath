//! Config aggregate root — pure domain types.
//!
//! No I/O, no env reads, no filesystem access.

use serde::{Deserialize, Serialize};

use crate::config::hooks::HooksConfig;
use crate::config::legacy::{ApiConfig, ModelConfig};
use crate::config::logging::LoggingConfig;
use crate::config::memory::MemoryConfig;
use crate::config::models::ModelsConfig;
use crate::config::permissions::PermissionConfig;
use crate::config::reasoning_graph::ReasoningGraphConfig;
use crate::config::skills::SkillsConfig;
use crate::config::storage::StorageConfig;
use crate::config::tools::{AgentsConfig, ToolsConfig};
use crate::config::ui::UiConfig;
use crate::config::update::UpdateConfig;

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

    /// Reasoning Graph configuration
    #[serde(default)]
    pub reasoning_graph: ReasoningGraphConfig,

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
            reasoning_graph: ReasoningGraphConfig::default(),
            guidance: GuidanceConfig::default(),
            update: UpdateConfig::default(),
            language: default_language(),
        }
    }
}
