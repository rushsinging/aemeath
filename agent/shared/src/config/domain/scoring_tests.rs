//! ScoringConfig 默认值、serde 与别名行为测试。

use super::*;

#[test]
fn default_scoring_config_disables_all_scenarios() {
    let config = ScoringConfig::default();
    assert_eq!(config.url, "http://127.0.0.1:8009");
    assert_eq!(config.model, "kev-latest");
    assert_eq!(config.timeout_ms, 2_000);
    assert!(!config.memory_rerank);
    assert!(!config.skill_match);
    assert!(!config.policy_triage);
    assert!(!config.memory_recall);
}

#[test]
fn scoring_config_deserializes_partial_json_with_defaults() {
    let config: ScoringConfig =
        serde_json::from_str(r#"{"url": "http://10.0.0.2:9000"}"#).expect("反序列化");
    assert_eq!(config.url, "http://10.0.0.2:9000");
    assert_eq!(config.model, "kev-latest");
    assert_eq!(config.timeout_ms, 2_000);
    assert!(!config.memory_rerank);
}

#[test]
fn scoring_config_accepts_camel_case_aliases() {
    let config: ScoringConfig = serde_json::from_str(
        r#"{"timeoutMs": 500, "memoryRerank": true, "skillMatch": true, "policyTriage": true}"#,
    )
    .expect("camelCase 别名应可解析");
    assert_eq!(config.timeout_ms, 500);
    assert!(config.memory_rerank);
    assert!(config.skill_match);
    assert!(config.policy_triage);
}

#[test]
fn empty_scoring_json_uses_defaults() {
    let config: ScoringConfig = serde_json::from_str(r#"{}"#).expect("空对象应解析");
    assert_eq!(config, ScoringConfig::default());
}
