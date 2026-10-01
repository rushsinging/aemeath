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
