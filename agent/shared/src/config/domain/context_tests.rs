//! `ContextConfig.compact_model` 的解析、别名与合并语义。

use crate::config::domain::config::Config;
use crate::config::domain::merge::{apply_patch, ConfigPatch};
use crate::config::domain::snapshot::ConfigSnapshot;

fn snapshot_from_json(source: &str) -> ConfigSnapshot {
    let config: Config = serde_json::from_str(source).expect("config must parse");
    ConfigSnapshot::new(config)
}

#[test]
fn compact_model_defaults_to_unset() {
    let config = Config::default();

    assert!(config.context.compact_model.is_none());
    assert!(ConfigSnapshot::new(config)
        .context_compact_model()
        .is_none());
}

#[test]
fn compact_model_parses_snake_case_field() {
    let snapshot = snapshot_from_json(r#"{"context":{"compact_model":"Zhipu/glm-5.3"}}"#);

    assert_eq!(snapshot.context_compact_model(), Some("Zhipu/glm-5.3"));
}

#[test]
fn compact_model_accepts_camel_case_alias() {
    let snapshot = snapshot_from_json(r#"{"context":{"compactModel":"Zhipu/glm-5.3"}}"#);

    assert_eq!(snapshot.context_compact_model(), Some("Zhipu/glm-5.3"));
}

#[test]
fn compact_model_rejects_invalid_type() {
    let parsed = serde_json::from_str::<Config>(r#"{"context":{"compact_model":123}}"#);

    assert!(parsed.is_err(), "compact_model 必须是字符串 selection");
}

#[test]
fn compact_model_patch_overrides_without_touching_other_context_fields() {
    let base = Config {
        context: crate::config::context::ContextConfig {
            snip_enabled: false,
            microcompact_enabled: false,
            auto_compact_failure_limit: 7,
            compact_model: Some("Zhipu/glm-5.3".to_string()),
        },
        ..Config::default()
    };
    let patch: ConfigPatch = serde_json::from_str(
        r#"{"context":{"compact_model":"LiteLLM/anthropic/claude-opus-4-7"}}"#,
    )
    .expect("patch must parse");

    let merged = apply_patch(base, patch);

    assert_eq!(
        merged.context.compact_model.as_deref(),
        Some("LiteLLM/anthropic/claude-opus-4-7")
    );
    assert!(!merged.context.snip_enabled);
    assert!(!merged.context.microcompact_enabled);
    assert_eq!(merged.context.auto_compact_failure_limit, 7);
}

#[test]
fn compact_model_patch_without_value_preserves_existing_selection() {
    let base = Config {
        context: crate::config::context::ContextConfig {
            compact_model: Some("Zhipu/glm-5.3".to_string()),
            ..Default::default()
        },
        ..Config::default()
    };
    let patch: ConfigPatch =
        serde_json::from_str(r#"{"context":{"snip_enabled":false}}"#).expect("patch must parse");

    let merged = apply_patch(base, patch);

    assert_eq!(
        merged.context.compact_model.as_deref(),
        Some("Zhipu/glm-5.3")
    );
}

#[test]
fn compact_model_patch_with_blank_value_clears_selection() {
    let base = Config {
        context: crate::config::context::ContextConfig {
            compact_model: Some("Zhipu/glm-5.3".to_string()),
            ..Default::default()
        },
        ..Config::default()
    };
    let patch: ConfigPatch =
        serde_json::from_str(r#"{"context":{"compact_model":""}}"#).expect("patch must parse");

    let merged = apply_patch(base, patch);

    assert!(merged.context.compact_model.is_none());
    assert!(ConfigSnapshot::new(merged)
        .context_compact_model()
        .is_none());
}

#[test]
fn snapshot_normalizes_whitespace_only_compact_model_to_unset() {
    let snapshot = snapshot_from_json(r#"{"context":{"compact_model":"   "}}"#);

    assert!(snapshot.context_compact_model().is_none());
}

#[test]
fn snapshot_trims_surrounding_whitespace_in_compact_model() {
    let snapshot = snapshot_from_json(r#"{"context":{"compact_model":"  Zhipu/glm-5.3  "}}"#);

    assert_eq!(snapshot.context_compact_model(), Some("Zhipu/glm-5.3"));
}
