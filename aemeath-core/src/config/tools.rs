//! 工具与代理配置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub(crate) fn default_max_tool_concurrency() -> usize {
    10
}

pub(crate) fn default_max_agent_concurrency() -> usize {
    4
}

/// Tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    /// Enable/disable specific tools
    #[serde(default)]
    pub enabled: Vec<String>,

    /// Disabled tools
    #[serde(default)]
    pub disabled: Vec<String>,

    /// Tool-specific configurations
    #[serde(default)]
    pub settings: HashMap<String, serde_json::Value>,

    /// Maximum number of concurrent tool executions (default: 10)
    #[serde(default = "default_max_tool_concurrency", rename = "maxConcurrency")]
    pub max_concurrency: usize,
}

/// Agent role configuration — binds a named agent role to a specific LLM.
///
/// Example in config.json:
/// ```json
/// { "agents": { "roles": { "coder": { "model": "deepseek/deepseek-chat", "description": "Writes and edits code" } } } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRoleConfig {
    /// LLM to use for this role, in "<source>/<model>" format (e.g. "deepseek/deepseek-chat").
    /// Resolved via ModelsConfig::find_model at runtime.
    #[serde(default, rename = "model")]
    pub model: String,

    /// Human-readable description of what this role does.
    /// Used to build the main LLM's system prompt so it knows which roles are available.
    #[serde(default, rename = "description")]
    pub description: String,

    /// Appended to the sub-agent system prompt for role-specific instructions.
    #[serde(default, rename = "systemSuffix")]
    pub system_suffix: Option<String>,

    /// Reasoning / thinking mode for sub-agents using this role.
    /// - `None` (default) — inherit from the main model's reasoning setting
    /// - `Some(true)` — force enable thinking for this role
    /// - `Some(false)` — force disable thinking for this role
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsConfig {
    /// Maximum number of concurrent sub-agent executions (default: 4)
    #[serde(default = "default_max_agent_concurrency", rename = "maxConcurrency")]
    pub max_concurrency: usize,

    /// Named agent roles, each optionally bound to a different LLM.
    ///
    /// When the `Agent` tool is called with `model` matching a role name,
    /// the role's LLM config is used. Otherwise `model` is treated as a
    /// "<source>/<model>" selection directly.
    #[serde(default)]
    pub roles: HashMap<String, AgentRoleConfig>,

    /// Default LLM for sub-agents when no model is specified.
    /// Format: "<source>/<model>". Falls back to the main agent's client if empty.
    #[serde(default, rename = "defaultModel")]
    pub default_model: String,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled: Vec::new(),
            disabled: Vec::new(),
            settings: HashMap::new(),
            max_concurrency: default_max_tool_concurrency(),
        }
    }
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            max_concurrency: default_max_agent_concurrency(),
            roles: HashMap::new(),
            default_model: String::new(),
        }
    }
}
