//! 工具与代理配置

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

pub(super) fn default_max_tool_concurrency() -> usize {
    10
}

pub(super) fn default_max_agent_concurrency() -> usize {
    4
}

pub(super) fn default_tool_result_threshold_chars() -> usize {
    50_000
}

pub(super) fn default_tool_result_preview_head_chars() -> usize {
    2_000
}

pub(super) fn default_tool_result_preview_tail_chars() -> usize {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultConfig {
    #[serde(default = "default_tool_result_threshold_chars")]
    pub threshold_chars: usize,
    #[serde(default = "default_tool_result_preview_head_chars")]
    pub preview_head_chars: usize,
    #[serde(default = "default_tool_result_preview_tail_chars")]
    pub preview_tail_chars: usize,
}

impl Default for ToolResultConfig {
    fn default() -> Self {
        Self {
            threshold_chars: default_tool_result_threshold_chars(),
            preview_head_chars: default_tool_result_preview_head_chars(),
            preview_tail_chars: default_tool_result_preview_tail_chars(),
        }
    }
}

/// Run-scoped tool allow/deny selection derived from merged configuration.
///
/// Tool identities use the same ASCII case-insensitive normalization as the
/// Tools registry. Empty and whitespace-only entries are ignored; duplicate
/// entries collapse deterministically. A non-empty enabled set is an allowlist,
/// while disabled always wins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSelection {
    allowlist_active: bool,
    enabled: BTreeSet<String>,
    disabled: BTreeSet<String>,
}

impl ToolSelection {
    pub fn new(enabled: &[String], disabled: &[String]) -> Self {
        let mut enabled = normalize_tool_names(enabled);
        let allowlist_active = !enabled.is_empty();
        let disabled = normalize_tool_names(disabled);
        enabled.retain(|name| !disabled.contains(name));
        Self {
            allowlist_active,
            enabled,
            disabled,
        }
    }

    pub fn allows(&self, name: &str) -> bool {
        let name = name.trim().to_ascii_lowercase();
        !name.is_empty()
            && !self.disabled.contains(&name)
            && (!self.allowlist_active || self.enabled.contains(&name))
    }

    pub fn enabled(&self) -> Vec<&str> {
        self.enabled.iter().map(String::as_str).collect()
    }

    pub fn disabled(&self) -> Vec<&str> {
        self.disabled.iter().map(String::as_str).collect()
    }
}

fn normalize_tool_names(names: &[String]) -> BTreeSet<String> {
    names
        .iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect()
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

    /// Oversized tool-result materialization policy.
    #[serde(default)]
    pub tool_result: ToolResultConfig,
}

/// Agent role configuration — binds a named agent role to a specific LLM.
///
/// Model protocol and capability settings are owned by the referenced model entry.
/// Unknown legacy role fields such as `reasoning` are ignored by serde.
///
/// Example in config.json:
/// ```json
/// { "agents": { "roles": { "coder": { "model": "deepseek/deepseek-chat", "description": "Writes and edits code" } } } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRoleConfig {
    /// Whether this role is available for main-agent dispatch.
    #[serde(default = "default_agent_role_enabled")]
    pub enabled: bool,

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

    /// Maximum output token budget for sub-agents using this role.
    /// `None` and `Some(0)` both inherit/default; `Some(n > 0)` overrides.
    #[serde(
        default,
        rename = "max_tokens",
        alias = "maxTokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_tokens: Option<u32>,

    /// Tool policy for sub runs bound to this role. `None` keeps the default
    /// sub tool set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<RolePolicyConfig>,
}

impl Default for AgentRoleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: String::new(),
            description: String::new(),
            system_suffix: None,
            max_tokens: None,
            policy: None,
        }
    }
}

/// Tool policy bound to a role: allowlist plus optional capability restriction.
///
/// Both fields are optional; an absent `policy` (or an empty allowlist, which
/// the Tools-layer compiler rejects) means the role keeps the default sub tool
/// set. Config-layer stores plain strings only — tool-name and capability-name
/// validation happens at Tools-layer compilation time.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolePolicyConfig {
    /// Tool-name allowlist; unlisted tools are invisible to the run and any
    /// call against them is denied via the catalog-miss path.
    #[serde(default, rename = "allowed_tools", alias = "allowedTools")]
    pub allowed_tools: Vec<String>,

    /// Capability-bit restriction intersected with the allowlist-derived
    /// capabilities at compile time.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Builtin role fallback consumed via [`AgentsConfig::merged_roles`].
///
/// Builtin roles carry a policy and a description only; `model` stays empty so
/// runtime resolves the sub model from `AgentsConfig::default_model` (or the
/// main client fallback) unless the user overrides the role in config.
fn builtin_agent_roles() -> Vec<(&'static str, AgentRoleConfig)> {
    fn policy_role(allowed_tools: &[&str], description: &str) -> AgentRoleConfig {
        AgentRoleConfig {
            description: description.to_string(),
            policy: Some(RolePolicyConfig {
                allowed_tools: allowed_tools.iter().map(|tool| tool.to_string()).collect(),
                capabilities: Vec::new(),
            }),
            ..AgentRoleConfig::default()
        }
    }
    vec![
        (
            "planner",
            policy_role(
                &[
                    "Read",
                    "Grep",
                    "Glob",
                    "WebSearch",
                    "WebFetch",
                    "TaskGet",
                    "TaskListGet",
                    "TaskLists",
                    "ToolSearch",
                ],
                "Planning and task breakdown; read-only plus web research",
            ),
        ),
        (
            "coder",
            policy_role(
                &[
                    "Read",
                    "Write",
                    "Edit",
                    "Glob",
                    "Grep",
                    "Bash",
                    "ToolSearch",
                    "Skill",
                ],
                "Implementation; read/write/execute, no agent dispatch",
            ),
        ),
        (
            "searcher",
            policy_role(
                &[
                    "Read",
                    "Grep",
                    "Glob",
                    "WebSearch",
                    "WebFetch",
                    "ToolSearch",
                ],
                "Local and web code retrieval",
            ),
        ),
        (
            "tester",
            policy_role(
                &[
                    "Read",
                    "Write",
                    "Edit",
                    "Bash",
                    "Grep",
                    "Glob",
                    "ToolSearch",
                ],
                "Test authoring and execution",
            ),
        ),
        (
            "reviewer",
            policy_role(
                &["Read", "Grep", "Glob", "WebSearch", "ToolSearch"],
                "Read-only review",
            ),
        ),
    ]
}

fn default_agent_role_enabled() -> bool {
    true
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
            tool_result: ToolResultConfig::default(),
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

impl AgentsConfig {
    /// Merge builtin roles with config-defined roles.
    ///
    /// A config entry with the same name replaces the builtin definition
    /// wholesale (no field-level merge), keeping builtin roles a pure fallback.
    /// Single source of truth for runtime role resolution and composition-time
    /// per-role tool profile assembly.
    pub fn merged_roles(&self) -> HashMap<String, AgentRoleConfig> {
        let mut merged: HashMap<String, AgentRoleConfig> = builtin_agent_roles()
            .into_iter()
            .map(|(name, role)| (name.to_string(), role))
            .collect();
        for (name, role) in &self.roles {
            merged.insert(name.clone(), role.clone());
        }
        merged
    }

    /// Resolve a role by name for a sub run: builtin fallback, enabled check,
    /// and empty-model fallback to `default_model`. Single source of truth
    /// consumed by runtime role resolution paths.
    pub fn resolve_role(&self, name: &str) -> Option<ResolvedRole> {
        let mut role = self.merged_roles().get(name)?.clone();
        if !role.enabled {
            return Some(ResolvedRole::Disabled);
        }
        if role.model.trim().is_empty() {
            role.model = self.default_model.clone();
        }
        Some(ResolvedRole::Role(role))
    }
}

/// Outcome of [`AgentsConfig::resolve_role`].
#[derive(Debug, Clone)]
pub enum ResolvedRole {
    /// Role resolved; `model` already carries the `default_model` fallback.
    Role(AgentRoleConfig),
    /// Role exists but is disabled.
    Disabled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_role_config_enabled_defaults_to_true() {
        let config: AgentRoleConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(config.enabled);
        assert!(AgentRoleConfig::default().enabled);
    }

    #[test]
    fn agent_role_config_ignores_legacy_reasoning_field() {
        let config: AgentRoleConfig = serde_json::from_str(r#"{ "reasoning": false }"#).unwrap();
        let serialized = serde_json::to_value(config).unwrap();

        assert!(serialized.get("reasoning").is_none());
    }

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
    fn role_policy_parses_allowlist_and_capabilities() {
        let config: AgentRoleConfig = serde_json::from_str(
            r#"{ "policy": { "allowed_tools": ["Read", "Grep"], "capabilities": ["ReadWorkspace"] } }"#,
        )
        .unwrap();
        let policy = config.policy.expect("policy parsed");
        assert_eq!(policy.allowed_tools, vec!["Read", "Grep"]);
        assert_eq!(policy.capabilities, vec!["ReadWorkspace"]);
    }

    #[test]
    fn role_policy_supports_camel_case_alias() {
        let config: AgentRoleConfig =
            serde_json::from_str(r#"{ "policy": { "allowedTools": ["Read"] } }"#).unwrap();
        assert_eq!(
            config.policy.expect("policy parsed").allowed_tools,
            vec!["Read"]
        );
    }

    #[test]
    fn role_policy_absent_by_default_and_skipped_when_serializing() {
        let config: AgentRoleConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(config.policy.is_none());
        let serialized = serde_json::to_value(&config).unwrap();
        assert!(serialized.get("policy").is_none());
    }

    #[test]
    fn merged_roles_contains_builtin_roles() {
        let merged = AgentsConfig::default().merged_roles();
        for name in ["planner", "coder", "searcher", "tester", "reviewer"] {
            assert!(merged.contains_key(name), "missing builtin role {name}");
            assert!(
                merged[name].policy.is_some(),
                "builtin role {name} must carry a policy"
            );
        }
    }

    #[test]
    fn merged_roles_config_overrides_builtin_wholesale() {
        let mut agents = AgentsConfig::default();
        agents.roles.insert(
            "coder".to_string(),
            AgentRoleConfig {
                model: "qwen/qwen3-coder".to_string(),
                ..AgentRoleConfig::default()
            },
        );
        let merged = agents.merged_roles();
        let coder = &merged["coder"];
        assert_eq!(coder.model, "qwen/qwen3-coder");
        assert!(
            coder.policy.is_none(),
            "config override must replace the builtin definition wholesale"
        );
    }

    #[test]
    fn merged_roles_keeps_non_builtin_custom_roles() {
        let mut agents = AgentsConfig::default();
        agents.roles.insert(
            "refactorer".to_string(),
            AgentRoleConfig {
                model: "x/y".to_string(),
                policy: Some(RolePolicyConfig {
                    allowed_tools: vec!["Read".to_string()],
                    capabilities: Vec::new(),
                }),
                ..AgentRoleConfig::default()
            },
        );
        let merged = agents.merged_roles();
        assert!(merged.contains_key("refactorer"));
        assert_eq!(merged.len(), 6); // 5 builtin + 1 custom
    }

    #[test]
    fn resolve_role_finds_builtin_and_applies_default_model_fallback() {
        let mut agents = AgentsConfig::default();
        agents.default_model = "deepseek/deepseek-chat".to_string();
        let ResolvedRole::Role(resolved) = agents
            .resolve_role("searcher")
            .expect("builtin role resolves")
        else {
            panic!("builtin role must resolve to Role");
        };
        assert_eq!(
            resolved.model, "deepseek/deepseek-chat",
            "empty builtin model falls back to default_model"
        );
        assert!(resolved.policy.is_some());

        // config 覆盖的 model 优先于 default_model
        agents.roles.insert(
            "searcher".to_string(),
            AgentRoleConfig {
                model: "qwen/qwen3".to_string(),
                ..AgentRoleConfig::default()
            },
        );
        let ResolvedRole::Role(resolved) =
            agents.resolve_role("searcher").expect("override resolves")
        else {
            panic!("overridden role must resolve to Role");
        };
        assert_eq!(resolved.model, "qwen/qwen3");
    }

    #[test]
    fn resolve_role_reports_missing_and_disabled() {
        let agents = AgentsConfig::default();
        assert!(agents.resolve_role("no-such-role").is_none());

        let mut agents = AgentsConfig::default();
        agents.roles.insert(
            "archived".to_string(),
            AgentRoleConfig {
                enabled: false,
                model: "x/y".to_string(),
                ..AgentRoleConfig::default()
            },
        );
        assert!(matches!(
            agents.resolve_role("archived"),
            Some(ResolvedRole::Disabled)
        ));
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
    fn tool_result_config_defaults_preserve_existing_materialization_behavior() {
        let tools: ToolsConfig = serde_json::from_str("{}").unwrap();

        assert_eq!(tools.tool_result.threshold_chars, 50_000);
        assert_eq!(tools.tool_result.preview_head_chars, 2_000);
        assert_eq!(tools.tool_result.preview_tail_chars, 500);
    }

    #[test]
    fn tool_result_config_accepts_snake_case_values() {
        let tools: ToolsConfig = serde_json::from_str(
            r#"{
                "tool_result": {
                    "threshold_chars": 12000,
                    "preview_head_chars": 900,
                    "preview_tail_chars": 300
                }
            }"#,
        )
        .unwrap();

        assert_eq!(tools.tool_result.threshold_chars, 12_000);
        assert_eq!(tools.tool_result.preview_head_chars, 900);
        assert_eq!(tools.tool_result.preview_tail_chars, 300);
    }

    #[test]
    fn test_agents_config_uses_snake_case_and_accepts_legacy_aliases() {
        let snake: AgentsConfig = serde_json::from_str(
            r#"{ "max_concurrency": 7, "default_model": "snake/model", "roles": { "coder": { "enabled": false, "system_suffix": "snake" } } }"#,
        )
        .unwrap();
        let legacy: AgentsConfig = serde_json::from_str(
            r#"{ "maxConcurrency": 8, "defaultModel": "legacy/model", "roles": { "coder": { "systemSuffix": "legacy" } } }"#,
        )
        .unwrap();

        assert!(!snake.roles["coder"].enabled);
        assert!(legacy.roles["coder"].enabled);
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
        assert_eq!(
            serialized["roles"]["coder"]["enabled"],
            serde_json::json!(false)
        );
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
