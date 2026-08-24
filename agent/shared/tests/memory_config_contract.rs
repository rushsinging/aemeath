use share::config::domain::merge::{apply_patch, ConfigPatch, MemoryConfigPatch};
use share::config::{Config, MemoryConfig};

#[test]
fn existing_memory_config_gets_compatible_injection_token_budget_default() {
    let config: MemoryConfig = serde_json::from_str(
        r#"{
            "enabled": true,
            "max_entries": 100,
            "similarity_threshold": 0.8,
            "inject_count": 5
        }"#,
    )
    .unwrap();

    assert_eq!(config.inject_token_budget, 300);
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

    assert_eq!(merged.memory.inject_token_budget, 123);
}
