//! 工具与代理配置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub(super) fn default_max_tool_concurrency() -> usize {
    10
}

pub(super) fn default_max_agent_concurrency() -> usize {
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
    #[serde(default = "default_max_tool_concurrency", alias = "maxConcurrency")]
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
    #[serde(default, alias = "systemSuffix")]
    pub system_suffix: Option<String>,

    /// Reasoning / thinking mode for sub-agents using this role.
    /// - `None` (default) — inherit from the main model's reasoning setting
    /// - `Some(true)` — force enable thinking for this role
    /// - `Some(false)` — force disable thinking for this role
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,

    /// Maximum output token budget for sub-agents using this role.
    /// `None` and `Some(0)` both inherit/default; `Some(n > 0)` overrides.
    #[serde(
        default,
        rename = "max_tokens",
        alias = "maxTokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_tokens: Option<u32>,
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsConfig {
    /// Maximum number of concurrent sub-agent executions (default: 4)
    #[serde(default = "default_max_agent_concurrency", alias = "maxConcurrency")]
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
    #[serde(default, alias = "defaultModel")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_role_config_max_tokens_snake_case() {
        let config: AgentRoleConfig = serde_json::from_str(r#"{ "max_tokens": 8192 }"#).unwrap();
        assert_eq!(config.max_tokens, Some(8192));
    }

    #[test]
    fn test_agent_role_config_max_tokens_zero_inherits() {
        let config: AgentRoleConfig = serde_json::from_str(r#"{ "max_tokens": 0 }"#).unwrap();
        assert_eq!(config.max_tokens, Some(0));
    }

    #[test]
    fn test_agent_role_config_max_tokens_default_none() {
        let config: AgentRoleConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(config.max_tokens, None);
    }

    #[test]
    fn test_agent_role_config_max_tokens_camel_case_alias() {
        let config: AgentRoleConfig = serde_json::from_str(r#"{ "maxTokens": 4096 }"#).unwrap();
        assert_eq!(config.max_tokens, Some(4096));
    }

    #[test]
    fn test_tools_config_uses_snake_case_and_accepts_legacy_alias() {
        let snake: ToolsConfig = serde_json::from_str(r#"{ "max_concurrency": 7 }"#).unwrap();
        let legacy: ToolsConfig = serde_json::from_str(r#"{ "maxConcurrency": 8 }"#).unwrap();

        assert_eq!(snake.max_concurrency, 7);
        assert_eq!(legacy.max_concurrency, 8);
        assert_eq!(
            serde_json::to_value(snake).unwrap()["max_concurrency"],
            serde_json::json!(7)
        );
    }

    #[test]
    fn test_agents_config_uses_snake_case_and_accepts_legacy_aliases() {
        let snake: AgentsConfig = serde_json::from_str(
            r#"{ "max_concurrency": 7, "default_model": "snake/model", "roles": { "coder": { "system_suffix": "snake" } } }"#,
        )
        .unwrap();
        let legacy: AgentsConfig = serde_json::from_str(
            r#"{ "maxConcurrency": 8, "defaultModel": "legacy/model", "roles": { "coder": { "systemSuffix": "legacy" } } }"#,
        )
        .unwrap();

        assert_eq!(snake.max_concurrency, 7);
        assert_eq!(snake.default_model, "snake/model");
        assert_eq!(snake.roles["coder"].system_suffix.as_deref(), Some("snake"));
        assert_eq!(legacy.max_concurrency, 8);
        assert_eq!(legacy.default_model, "legacy/model");
        assert_eq!(
            legacy.roles["coder"].system_suffix.as_deref(),
            Some("legacy")
        );

        let serialized = serde_json::to_value(snake).unwrap();
        assert_eq!(serialized["max_concurrency"], serde_json::json!(7));
        assert_eq!(
            serialized["default_model"],
            serde_json::json!("snake/model")
        );
        assert_eq!(
            serialized["roles"]["coder"]["system_suffix"],
            serde_json::json!("snake")
        );
    }
}
