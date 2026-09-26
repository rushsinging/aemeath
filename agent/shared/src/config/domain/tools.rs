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

/// Agent role definition — the policy-holding half of the role/instance split.
///
/// A role owns the tool policy and a fallback description; concrete model
/// bindings live in [`AgentInstanceConfig`] entries under `agents.names`.
/// `deny_unknown_fields` rejects the retired flat format (role entries that
/// carried `model`/`enabled` instance fields) with a serde error at parse time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct AgentRoleDefinition {
    /// Human-readable description of what this role does; used as fallback
    /// when a named instance carries no description of its own.
    #[serde(default)]
    pub description: String,

    /// Tool policy for runs dispatched under this role. `None` keeps the
    /// default sub tool set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<RolePolicyConfig>,
}

/// Named agent instance — the model-holding half of the role/instance split.
///
/// Example in config.json:
/// ```json
/// { "agents": { "names": { "reviewer-glm": { "role": "reviewer", "model": "Zhipu/glm-5.2" } } } }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstanceConfig {
    /// Name of the role this instance fulfills; MUST resolve against builtin
    /// or config-defined `agents.roles`, otherwise config validation fails.
    #[serde(default)]
    pub role: String,

    /// LLM for this instance, in "<source>/<model>" format; empty falls back
    /// to `AgentsConfig::default_model`. Resolved via ModelsConfig::find_model.
    #[serde(default)]
    pub model: String,

    /// Whether this instance is available for main-agent dispatch.
    #[serde(default = "default_agent_role_enabled")]
    pub enabled: bool,

    /// Instance-level description shown in the main LLM's role list; falls
    /// back to the referenced role's description when empty.
    #[serde(default)]
    pub description: String,

    /// Appended to the sub-agent system prompt for instance-specific instructions.
    #[serde(default, alias = "systemSuffix")]
    pub system_suffix: Option<String>,

    /// Maximum output token budget for sub-agents using this instance.
    /// `None` and `Some(0)` both inherit/default; `Some(n > 0)` overrides.
    #[serde(
        default,
        rename = "max_tokens",
        alias = "maxTokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_tokens: Option<u32>,
}

impl Default for AgentInstanceConfig {
    fn default() -> Self {
        Self {
            role: String::new(),
            model: String::new(),
            enabled: true,
            description: String::new(),
            system_suffix: None,
            max_tokens: None,
        }
    }
}

/// Tool policy bound to a role: capability-only allow-set.
///
/// The role's toolset is assembled from the declared capability groups
/// (capability → tool-group mapping is owned by the Tools layer). An absent
/// `policy` (or empty `capabilities`) means the role keeps the default sub
/// tool set. Config-layer stores plain strings only — capability-name
/// validation happens at Tools-layer compilation time.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolePolicyConfig {
    /// Capability allow-set; tools outside the declared groups are invisible
    /// to the run and any call against them is denied via the catalog-miss
    /// path.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Builtin role definitions consumed via [`AgentsConfig::merged_roles`].
///
/// Builtin roles are declared as capability groups; model bindings live in
/// named instances under `agents.names`. Capability → tool-group mapping is
/// owned by the Tools layer.
fn builtin_agent_roles() -> Vec<(&'static str, AgentRoleDefinition)> {
    fn capability_role(capabilities: &[&str], description: &str) -> AgentRoleDefinition {
        AgentRoleDefinition {
            description: description.to_string(),
            policy: Some(RolePolicyConfig {
                capabilities: capabilities.iter().map(|cap| cap.to_string()).collect(),
            }),
        }
    }
    vec![
        (
            "planner",
            capability_role(
                &["Read", "NetworkAccess", "TaskRead", "TaskWrite"],
                "Planning and task breakdown; read-only plus web research and task writes",
            ),
        ),
        (
            "coder",
            capability_role(
                &["Read", "Write", "Execute"],
                "Implementation; read/write/execute, no agent dispatch",
            ),
        ),
        (
            "explorer",
            capability_role(&["Read", "NetworkAccess"], "Local and web code retrieval"),
        ),
        (
            "tester",
            capability_role(
                &["Read", "Write", "Execute"],
                "Test authoring and execution",
            ),
        ),
        ("reviewer", capability_role(&["Read"], "Read-only review")),
    ]
}

fn default_agent_role_enabled() -> bool {
    true
}

/// Agent configuration: role definitions (policy) + named instances (models).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsConfig {
    /// Maximum number of concurrent sub-agent executions (default: 4)
    #[serde(default = "default_max_agent_concurrency", alias = "maxConcurrency")]
    pub max_concurrency: usize,

    /// Role definitions: the policy-holding half. Config entries with the
    /// same name replace builtin definitions wholesale.
    #[serde(default)]
    pub roles: HashMap<String, AgentRoleDefinition>,

    /// Named agent instances: the model-holding half. Each entry references
    /// a role by name and binds a concrete LLM plus instance-level hints.
    #[serde(default)]
    pub names: HashMap<String, AgentInstanceConfig>,

    /// Default LLM for instances that omit `model`.
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
            names: HashMap::new(),
            default_model: String::new(),
        }
    }
}

impl AgentsConfig {
    /// Merge builtin role definitions with config-defined ones.
    ///
    /// A config entry with the same name replaces the builtin definition
    /// wholesale (no field-level merge). Single source of truth for runtime
    /// resolution and composition-time per-role tool profile assembly.
    pub fn merged_roles(&self) -> HashMap<String, AgentRoleDefinition> {
        let mut merged: HashMap<String, AgentRoleDefinition> = builtin_agent_roles()
            .into_iter()
            .map(|(name, role)| (name.to_string(), role))
            .collect();
        for (name, role) in &self.roles {
            merged.insert(name.clone(), role.clone());
        }
        merged
    }

    /// 已启用实例名（排序），用于错误提示与 roster 的可用名单单一口径。
    pub fn enabled_instance_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .names
            .iter()
            .filter(|(_, instance)| instance.enabled)
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        names
    }

    /// Resolve a named agent instance for a sub run: instance lookup, enabled
    /// check, role-reference validation, and empty-model fallback to
    /// `default_model`. Single source of truth consumed by runtime dispatch.
    pub fn resolve_agent(&self, name: &str) -> Option<ResolveAgentOutcome> {
        let instance = self.names.get(name)?;
        if !instance.enabled {
            return Some(ResolveAgentOutcome::Disabled {
                instance_name: name.to_string(),
            });
        }
        let role_name = instance.role.trim().to_string();
        let role = self.merged_roles().get(&role_name)?.clone();
        let model = if instance.model.trim().is_empty() {
            self.default_model.clone()
        } else {
            instance.model.clone()
        };
        let description = if instance.description.is_empty() {
            role.description.clone()
        } else {
            instance.description.clone()
        };
        Some(ResolveAgentOutcome::Agent(ResolvedAgent {
            instance_name: name.to_string(),
            role_name,
            model,
            description,
            system_suffix: instance.system_suffix.clone(),
            max_tokens: instance.max_tokens,
            policy: role.policy,
        }))
    }
}

/// Outcome of [`AgentsConfig::resolve_agent`].
#[derive(Debug, Clone)]
pub enum ResolveAgentOutcome {
    /// Instance resolved; `model` already carries the `default_model` fallback.
    Agent(ResolvedAgent),
    /// Instance exists but is disabled.
    Disabled { instance_name: String },
}

/// Fully resolved named agent: instance hints plus its role's policy.
#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    /// The `agents.names` key this resolution started from.
    pub instance_name: String,
    /// The referenced role's name (used for `role:<name>` profile selection).
    pub role_name: String,
    /// Effective model, with `default_model` fallback applied.
    pub model: String,
    /// Instance description with the role description as fallback.
    pub description: String,
    pub system_suffix: Option<String>,
    pub max_tokens: Option<u32>,
    /// The referenced role's tool policy, if any.
    pub policy: Option<RolePolicyConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_instance_config_defaults_are_dispatchable() {
        let config: AgentInstanceConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(config.enabled);
        assert!(AgentInstanceConfig::default().enabled);
        assert_eq!(config.role, "");
        assert_eq!(config.model, "");
    }

    #[test]
    fn enabled_instance_names_lists_only_enabled_instances_sorted() {
        let mut agents = AgentsConfig::default();
        for (name, enabled) in [("zeta", true), ("alpha", true), ("mid", false)] {
            agents.names.insert(
                name.to_string(),
                AgentInstanceConfig {
                    enabled,
                    ..Default::default()
                },
            );
        }

        assert_eq!(
            agents.enabled_instance_names(),
            vec!["alpha".to_string(), "zeta".to_string()],
            "可用名单只含 enabled 实例且排序稳定"
        );
    }

    #[test]
    fn role_definition_rejects_retired_flat_format_with_instance_fields() {
        // 旧扁平格式（role 条目带 model/enabled 等实例字段）必须被
        // deny_unknown_fields 直接拒绝——breaking，无读时迁移。
        let result: Result<AgentRoleDefinition, _> =
            serde_json::from_str(r#"{ "model": "x/y", "enabled": true }"#);
        assert!(result.is_err(), "flat role format must be rejected");
    }

    #[test]
    fn role_policy_parses_capabilities_only() {
        let config: AgentRoleDefinition =
            serde_json::from_str(r#"{ "policy": { "capabilities": ["Read", "Write"] } }"#).unwrap();
        assert_eq!(
            config.policy.expect("policy parsed").capabilities,
            vec!["Read", "Write"]
        );
    }

    #[test]
    fn builtin_planner_declares_task_write_capability() {
        let merged = AgentsConfig::default().merged_roles();
        let planner = &merged["planner"];
        assert_eq!(
            planner
                .policy
                .as_ref()
                .expect("planner policy")
                .capabilities,
            vec!["Read", "NetworkAccess", "TaskRead", "TaskWrite"]
        );
    }

    #[test]
    fn merged_roles_contains_builtin_definitions() {
        let merged = AgentsConfig::default().merged_roles();
        for name in ["planner", "coder", "explorer", "tester", "reviewer"] {
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
            AgentRoleDefinition {
                description: "custom coder".to_string(),
                policy: None,
            },
        );
        let merged = agents.merged_roles();
        let coder = &merged["coder"];
        assert_eq!(coder.description, "custom coder");
        assert!(
            coder.policy.is_none(),
            "config override must replace the builtin definition wholesale"
        );
    }

    fn instance(role: &str, model: &str) -> AgentInstanceConfig {
        AgentInstanceConfig {
            role: role.to_string(),
            model: model.to_string(),
            ..AgentInstanceConfig::default()
        }
    }

    #[test]
    fn resolve_agent_joins_instance_with_role_policy() {
        let mut agents = AgentsConfig::default();
        agents.names.insert(
            "reviewer-glm".to_string(),
            instance("reviewer", "Zhipu/glm-5.2"),
        );
        let ResolveAgentOutcome::Agent(resolved) = agents
            .resolve_agent("reviewer-glm")
            .expect("instance resolves")
        else {
            panic!("named instance must resolve to Agent");
        };
        assert_eq!(resolved.role_name, "reviewer");
        assert_eq!(resolved.model, "Zhipu/glm-5.2");
        assert!(
            resolved.policy.is_some(),
            "builtin reviewer policy is joined in"
        );
    }

    #[test]
    fn resolve_agent_applies_default_model_fallback() {
        let mut agents = AgentsConfig {
            default_model: "Zhipu/glm-5.3".to_string(),
            ..AgentsConfig::default()
        };
        agents
            .names
            .insert("coder-fast".to_string(), instance("coder", ""));
        let ResolveAgentOutcome::Agent(resolved) = agents
            .resolve_agent("coder-fast")
            .expect("instance resolves")
        else {
            panic!("named instance must resolve to Agent");
        };
        assert_eq!(resolved.model, "Zhipu/glm-5.3");
    }

    #[test]
    fn resolve_agent_reports_missing_disabled_and_unknown_role() {
        let agents = AgentsConfig::default();
        assert!(agents.resolve_agent("no-such-agent").is_none());

        let mut agents = AgentsConfig::default();
        let mut disabled = instance("coder", "x/y");
        disabled.enabled = false;
        agents.names.insert("archived".to_string(), disabled);
        assert!(matches!(
            agents.resolve_agent("archived"),
            Some(ResolveAgentOutcome::Disabled { .. })
        ));

        // 实例引用未定义职能（非内置名且 config roles 未定义）→ 视为未解析
        agents
            .names
            .insert("ghost".to_string(), instance("no-such-role", "x/y"));
        assert!(agents.resolve_agent("ghost").is_none());
    }

    #[test]
    fn resolve_agent_instance_description_wins_over_role_fallback() {
        let mut agents = AgentsConfig::default();
        let mut described = instance("reviewer", "x/y");
        described.description = "Reviews code (GLM)".to_string();
        agents.names.insert("reviewer-glm".to_string(), described);
        let ResolveAgentOutcome::Agent(resolved) = agents
            .resolve_agent("reviewer-glm")
            .expect("instance resolves")
        else {
            panic!("named instance must resolve to Agent");
        };
        assert_eq!(resolved.description, "Reviews code (GLM)");

        agents
            .names
            .insert("reviewer-ds".to_string(), instance("reviewer", "x/y"));
        let ResolveAgentOutcome::Agent(fallback) = agents
            .resolve_agent("reviewer-ds")
            .expect("instance resolves")
        else {
            panic!("named instance must resolve to Agent");
        };
        assert!(
            !fallback.description.is_empty(),
            "builtin role description fills in"
        );
    }

    #[test]
    fn agents_config_accepts_snake_case_and_legacy_aliases() {
        let config: AgentsConfig =
            serde_json::from_str(r#"{ "maxConcurrency": 2, "defaultModel": "a/b" }"#).unwrap();
        assert_eq!(config.max_concurrency, 2);
        assert_eq!(config.default_model, "a/b");
        assert!(config.roles.is_empty());
        assert!(config.names.is_empty());
    }

    #[test]
    fn tool_result_config_defaults_preserve_existing_materialization_behavior() {
        let config: ToolsConfig = ToolsConfig::default();
        assert!(config.enabled.is_empty());
        assert!(config.disabled.is_empty());
        assert_eq!(config.max_concurrency, default_max_tool_concurrency());
    }
}
