use share::config::domain::merge::{apply_patch, ConfigPatch, MemoryConfigPatch};
use share::config::{Config, MemoryConfig};

/// #1777：默认预算改为按窗口比例（`None`），不再有固定默认值。
#[test]
fn the_default_injection_budget_follows_the_window_ratio() {
    let config = MemoryConfig::default();
    assert_eq!(
        config.inject_token_budget, None,
        "the default must be ratio-derived, not a fixed token count"
    );
}

/// #1777：旧配置里的 `inject_count` 残留被忽略，而不是报错。
#[test]
fn a_legacy_inject_count_is_ignored_rather_than_rejected() {
    let config: MemoryConfig = serde_json::from_str(
        r#"{
            "enabled": true,
            "max_entries": 100,
            "similarity_threshold": 0.8,
            "inject_count": 5,
            "inject_token_budget": 420
        }"#,
    )
    .expect("a config carrying the retired inject_count must still load");

    assert_eq!(
        config.inject_token_budget,
        Some(420),
        "an explicit override from the old format is still honoured"
    );
}

/// 显式 `0` 保留「禁用自动注入」的能力，区别于「按比例」。
#[test]
fn an_explicit_zero_budget_disables_injection_and_differs_from_the_ratio_default() {
    let disabled: MemoryConfig =
        serde_json::from_str(r#"{"enabled": true, "inject_token_budget": 0}"#).unwrap();
    assert_eq!(disabled.inject_token_budget, Some(0));
    assert_ne!(
        disabled.inject_token_budget,
        MemoryConfig::default().inject_token_budget
    );
}

#[test]
fn memory_injection_token_budget_participates_in_sparse_patch_merge() {
    let base = Config::default();
    let merged = apply_patch(
        base,
        ConfigPatch {
            memory: Some(MemoryConfigPatch {
                inject_token_budget: Some(123),
                ..MemoryConfigPatch::default()
            }),
            ..ConfigPatch::default()
        },
    );

    assert_eq!(merged.memory.inject_token_budget, Some(123));
}

/// 旧 patch 携带 `inject_count` 时被容忍并忽略，不影响合并结果。
#[test]
fn a_legacy_patch_carrying_inject_count_merges_without_effect() {
    let merged = apply_patch(
        Config::default(),
        ConfigPatch {
            memory: Some(MemoryConfigPatch {
                inject_count: Some(9),
                ..MemoryConfigPatch::default()
            }),
            ..ConfigPatch::default()
        },
    );

    assert_eq!(merged.memory.inject_token_budget, None);
}
