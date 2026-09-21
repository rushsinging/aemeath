use share::config::domain::merge::{apply_patch, ConfigPatch};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

#[test]
fn context_read_reduction_defaults_are_enabled() {
    let config = Config::default();
    let snapshot = ConfigSnapshot::new(config.clone());

    assert!(config.context.snip_enabled);
    assert!(config.context.microcompact_enabled);
    assert!(snapshot.context_snip_enabled());
    assert!(snapshot.context_microcompact_enabled());
}

#[test]
fn context_read_reduction_json_fields_are_semantic_and_independent() {
    let snip_disabled: Config = serde_json::from_value(serde_json::json!({
        "context": {
            "snip_enabled": false
        }
    }))
    .unwrap();
    let microcompact_disabled: Config = serde_json::from_value(serde_json::json!({
        "context": {
            "microcompact_enabled": false
        }
    }))
    .unwrap();

    assert!(!snip_disabled.context.snip_enabled);
    assert!(snip_disabled.context.microcompact_enabled);
    assert!(microcompact_disabled.context.snip_enabled);
    assert!(!microcompact_disabled.context.microcompact_enabled);
}

#[test]
fn sparse_context_patch_preserves_unspecified_switch() {
    let patch: ConfigPatch = serde_json::from_value(serde_json::json!({
        "context": {
            "snip_enabled": false
        }
    }))
    .unwrap();
    let merged = apply_patch(Config::default(), patch);
    let snapshot = ConfigSnapshot::new(merged);

    assert!(!snapshot.context_snip_enabled());
    assert!(snapshot.context_microcompact_enabled());
}
