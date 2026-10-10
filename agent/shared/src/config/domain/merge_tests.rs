use super::*;
use crate::config::domain::snapshot::ConfigSnapshot;
use crate::config::ui::MarkdownSpacingMode;

#[test]
fn storage_worktrees_dir_patch_overrides_lower_layer_value() {
    let global: ConfigPatch =
        serde_json::from_str(r#"{"storage":{"worktrees_dir":"/global/wt"}}"#).unwrap();
    let env_layer = ConfigPatch {
        storage: Some(StorageConfigPatch {
            worktrees_dir: Some(PathBuf::from("/env/wt")),
            ..Default::default()
        }),
        ..Default::default()
    };

    let config = apply_patch(apply_patch(Config::default(), global), env_layer);
    let snapshot = ConfigSnapshot::new(config);

    assert_eq!(
        snapshot.worktrees_dir(),
        Some(PathBuf::from("/env/wt").as_path())
    );
}

#[test]
fn scoring_patch_overrides_only_set_fields_and_reaches_snapshot() {
    // 退役 HTTP 键（url）在旧配置文件残留时必须被忽略而非报错（设计 §4.3）。
    let global: ConfigPatch =
        serde_json::from_str(r#"{"scoring":{"url":"http://global:8009","memoryRerank":true}}"#)
            .unwrap();
    let env_layer = ConfigPatch {
        scoring: Some(ScoringConfigPatch {
            skill_match: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };

    let config = apply_patch(apply_patch(Config::default(), global), env_layer);
    let snapshot = ConfigSnapshot::new(config);

    let scoring = snapshot.scoring();
    assert!(scoring.memory_rerank, "低层开关应保留");
    assert!(scoring.skill_match, "高层字段应覆盖低层缺省");
    assert!(!scoring.policy_triage);
    assert!(!scoring.memory_recall);

    let value = serde_json::to_value(scoring).expect("ScoringConfig 应可序列化");
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("对象")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "enabled",
            "memory_recall",
            "memory_rerank",
            "policy_triage",
            "skill_match"
        ],
        "合并结果不得出现退役 HTTP 字段"
    );
}

#[test]
fn scoring_explicit_env_false_overrides_lower_layer_true() {
    let global: ConfigPatch = serde_json::from_str(r#"{"scoring":{"policyTriage":true}}"#).unwrap();
    let env_layer = ConfigPatch {
        scoring: Some(ScoringConfigPatch {
            policy_triage: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };

    let config = apply_patch(apply_patch(Config::default(), global), env_layer);

    assert!(
        !ConfigSnapshot::new(config).scoring().policy_triage,
        "显式 false 必须覆盖低层 true"
    );
}

#[test]
fn storage_worktrees_dir_defaults_to_none_without_patch() {
    let snapshot = ConfigSnapshot::new(Config::default());

    assert_eq!(snapshot.worktrees_dir(), None);
}

#[test]
fn hook_env_passthrough_patch_overrides_and_inherits_when_unset() {
    let global: ConfigPatch =
        serde_json::from_str(r#"{"hooks": {"env_passthrough": ["CMUX_*"]}}"#).unwrap();
    let project_override: ConfigPatch =
        serde_json::from_str(r#"{"hooks": {"env_passthrough": ["SSH_AUTH_SOCK"]}}"#).unwrap();
    let project_unset: ConfigPatch =
        serde_json::from_str(r#"{"hooks": {"max_attempts": 3}}"#).unwrap();

    let merged_global_only = apply_patch(Config::default(), global.clone());
    assert_eq!(
        merged_global_only.hooks.env_passthrough,
        vec!["CMUX_*".to_string()]
    );

    // overlay 设置了透传模式：整体覆盖（不做列表拼接合并）
    let merged_override = apply_patch(
        apply_patch(Config::default(), global.clone()),
        project_override,
    );
    assert_eq!(
        merged_override.hooks.env_passthrough,
        vec!["SSH_AUTH_SOCK".to_string()]
    );

    // overlay 未设置（空）：继承全局层
    let merged_inherit = apply_patch(apply_patch(Config::default(), global), project_unset);
    assert_eq!(
        merged_inherit.hooks.env_passthrough,
        vec!["CMUX_*".to_string()]
    );
}

#[test]
fn hook_runtime_limit_patch_preserves_unspecified_lower_layer_values() {
    let global: ConfigPatch = serde_json::from_str(
        r#"{
                "hooks": {
                    "max_attempts": 2,
                    "max_stop_hook_blocks": 4
                }
            }"#,
    )
    .unwrap();
    let project: ConfigPatch = serde_json::from_str(
        r#"{
                "hooks": {
                    "max_attempts": 5,
                    "PreToolUse": []
                }
            }"#,
    )
    .unwrap();

    let config = apply_patch(apply_patch(Config::default(), global), project);
    let snapshot = ConfigSnapshot::new(config);

    assert_eq!(snapshot.hook_execution_policy().max_attempts(), 5);
    assert_eq!(snapshot.stop_hook_policy().max_blocks(), 4);
    assert!(snapshot
        .hooks()
        .events
        .contains_key(&crate::config::hooks::HookEvent::PreToolUse));
}

#[test]
fn test_config_patch_snake_case_concurrency_reaches_snapshot() {
    let patch: ConfigPatch = serde_json::from_str(
        r#"{
                "tools": { "max_concurrency": 9 },
                "agents": {
                    "max_concurrency": 6,
                    "default_model": "snake/model",
                    "names": {
                        "coder-fast": {
                            "role": "coder",
                            "model": "snake/model",
                            "enabled": false,
                            "system_suffix": "snake"
                        }
                    }
                }
            }"#,
    )
    .unwrap();

    let snapshot = ConfigSnapshot::new(apply_patch(Config::default(), patch));

    assert!(!snapshot.agents().names["coder-fast"].enabled);
    assert_eq!(snapshot.max_tool_concurrency(), 9);
    assert_eq!(snapshot.max_agent_concurrency(), 6);
    assert_eq!(snapshot.agents().default_model, "snake/model");
    assert_eq!(
        snapshot.agents().names["coder-fast"]
            .system_suffix
            .as_deref(),
        Some("snake")
    );
}

#[test]
fn test_config_patch_accepts_legacy_agent_and_tool_aliases() {
    let patch: ConfigPatch = serde_json::from_str(
        r#"{
                "tools": { "maxConcurrency": 8 },
                "agents": { "maxConcurrency": 5, "defaultModel": "legacy/model" }
            }"#,
    )
    .unwrap();

    let snapshot = ConfigSnapshot::new(apply_patch(Config::default(), patch));

    assert_eq!(snapshot.max_tool_concurrency(), 8);
    assert_eq!(snapshot.max_agent_concurrency(), 5);
    assert_eq!(snapshot.agents().default_model, "legacy/model");
}

#[test]
fn tool_result_partial_patch_preserves_unspecified_policy_fields() {
    let patch: ConfigPatch = serde_json::from_str(
        r#"{
                "tools": {
                    "tool_result": { "threshold_chars": 9000 }
                }
            }"#,
    )
    .unwrap();

    let snapshot = ConfigSnapshot::new(apply_patch(Config::default(), patch));
    // 窗口未知（0）不按比例收紧：验证的就是 patch 后的原始策略值，
    // 若改用大窗口，比例收紧结果会掩盖「未指定字段保留默认值」的失效。
    let policy = snapshot.tool_result_policy(0);

    assert_eq!(policy.threshold_chars(), 9_000);
    assert_eq!(policy.preview_head_chars(), 2_000);
    assert_eq!(policy.preview_tail_chars(), 500);
}

#[test]
fn auto_compact_failure_limit_patch_preserves_unspecified_context_values() {
    let base = Config {
        context: ContextConfig {
            snip_enabled: false,
            auto_compact_failure_limit: 7,
            ..ContextConfig::default()
        },
        ..Config::default()
    };
    let merged = apply_patch(
        base,
        ConfigPatch {
            context: Some(ContextConfigPatch {
                auto_compact_failure_limit: Some(2),
                ..ContextConfigPatch::default()
            }),
            ..ConfigPatch::default()
        },
    );

    assert!(!merged.context.snip_enabled);
    assert_eq!(merged.context.auto_compact_failure_limit, 2);
}

#[test]
fn markdown_spacing_patch_merges_element_edges_sparsely() {
    let global: ConfigPatch = serde_json::from_str(
        r#"{
                "ui": {
                    "markdown_spacing": "compact",
                    "markdown_spacing_overrides": {
                        "heading": { "before": 1, "after": 2 },
                        "paragraph": { "after": 1 }
                    }
                }
            }"#,
    )
    .unwrap();
    let local: ConfigPatch = serde_json::from_str(
        r#"{
                "ui": {
                    "markdown_spacing_overrides": {
                        "heading": { "after": 0 }
                    }
                }
            }"#,
    )
    .unwrap();

    let mut chain = PriorityChain::new();
    chain.push(global);
    chain.push(local);
    let snapshot = ConfigSnapshot::new(chain.merge(Config::default()));
    let overrides = snapshot.markdown_spacing_overrides();

    assert_eq!(
        snapshot.markdown_spacing_mode(),
        MarkdownSpacingMode::Compact
    );
    assert_eq!(overrides.heading.unwrap().before.unwrap().get(), 1);
    assert_eq!(overrides.heading.unwrap().after.unwrap().get(), 0);
    assert_eq!(overrides.paragraph.unwrap().after.unwrap().get(), 1);
}

#[test]
fn runtime_patch_overrides_threshold_and_reaches_snapshot() {
    let global: ConfigPatch =
        serde_json::from_str(r#"{"runtime":{"tool_background_threshold_secs":30}}"#).unwrap();
    let env_layer = ConfigPatch {
        runtime: Some(RuntimeConfigPatch {
            tool_background_threshold_secs: Some(45),
        }),
        ..Default::default()
    };

    let config = apply_patch(apply_patch(Config::default(), global), env_layer);
    let snapshot = ConfigSnapshot::new(config);

    assert_eq!(
        snapshot.tool_background_threshold_secs(),
        45,
        "高层字段应覆盖"
    );
}

#[test]
fn runtime_patch_zero_disables_backgrounding() {
    let layer: ConfigPatch =
        serde_json::from_str(r#"{"runtime":{"tool_background_threshold_secs":0}}"#).unwrap();

    let config = apply_patch(Config::default(), layer);
    let snapshot = ConfigSnapshot::new(config);

    assert_eq!(
        snapshot.tool_background_threshold_secs(),
        0,
        "显式 0 表示禁用后台化，不得回退默认值"
    );
}

#[test]
fn runtime_patch_some_makes_config_patch_non_empty() {
    let layer = ConfigPatch {
        runtime: Some(RuntimeConfigPatch {
            tool_background_threshold_secs: Some(10),
        }),
        ..Default::default()
    };
    assert!(!layer.is_empty(), "runtime Some 时 is_empty 必须为 false");
}

#[test]
fn reflection_timeout_secs_patch_overrides_and_reaches_config() {
    let layer: ConfigPatch =
        serde_json::from_str(r#"{"memory":{"reflection":{"timeout_secs":600}}}"#).unwrap();

    let config = apply_patch(Config::default(), layer);

    assert_eq!(
        config.memory.reflection.timeout_secs, 600,
        "高层 patch 的 timeout_secs 必须覆盖缺省 240"
    );
    assert_eq!(
        config.memory.reflection.interval_runs, 10,
        "未设置字段保持缺省，不受本 patch 影响"
    );
}

#[test]
fn reflection_timeout_secs_unspecified_in_patch_preserves_lower_layer() {
    let global: ConfigPatch =
        serde_json::from_str(r#"{"memory":{"reflection":{"timeout_secs":600}}}"#).unwrap();
    let overlay: ConfigPatch =
        serde_json::from_str(r#"{"memory":{"reflection":{"interval_runs":3}}}"#).unwrap();

    let config = apply_patch(apply_patch(Config::default(), global), overlay);

    assert_eq!(
        config.memory.reflection.timeout_secs, 600,
        "overlay 未触碰 timeout_secs，必须保留低层值"
    );
    assert_eq!(config.memory.reflection.interval_runs, 3);
}
