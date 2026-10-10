//! ScoringConfig 默认值、serde 与别名行为测试。
//!
//! HTTP 端点配置（url/model/timeout_ms）已随设计 §4.3 退役：仅保留四个场景开关，
//! 旧键残留配置必须被忽略而非报错。

use super::*;

#[test]
fn default_scoring_config_disables_all_scenarios() {
    let config = ScoringConfig::default();
    assert!(!config.memory_rerank);
    assert!(!config.skill_match);
    assert!(!config.policy_triage);
    assert!(!config.memory_recall);
}

#[test]
fn scoring_config_ignores_retired_http_keys() {
    let config: ScoringConfig = serde_json::from_str(
        r#"{"url":"http://10.0.0.2:9000","model":"kev-0.8b","timeoutMs":500,"memoryRerank":true}"#,
    )
    .expect("退役 HTTP 键应被忽略而非报错");
    assert!(config.memory_rerank, "仍生效的开关应正常解析");
    assert!(!config.skill_match);

    let value = serde_json::to_value(&config).expect("ScoringConfig 应可序列化");
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
            "event_retention_days",
            "memory_recall",
            "memory_rerank",
            "policy_triage",
            "skill_match"
        ],
        "序列化面只允许场景开关与事件保留天数，HTTP 端点字段不得回归"
    );
}

#[test]
fn scoring_config_accepts_camel_case_aliases_for_switches() {
    let config: ScoringConfig = serde_json::from_str(
        r#"{"memoryRerank": true, "skillMatch": true, "policyTriage": true, "memoryRecall": true}"#,
    )
    .expect("camelCase 别名应可解析");
    assert!(config.memory_rerank);
    assert!(config.skill_match);
    assert!(config.policy_triage);
    assert!(config.memory_recall);
}

#[test]
fn empty_scoring_json_uses_defaults() {
    let config: ScoringConfig = serde_json::from_str(r#"{}"#).expect("空对象应解析");
    assert_eq!(config, ScoringConfig::default());
}

#[test]
fn event_retention_days_parses_from_scoring_section() {
    // 设计 03-event-stream §8：`scoring.event_retention_days` 配置贯通。
    let config: crate::config::Config =
        serde_json::from_str(r#"{"scoring":{"event_retention_days":7}}"#)
            .expect("scoring 配置段应可解析 event_retention_days");
    assert_eq!(config.scoring.event_retention_days, 7, "显式配置应生效");
}

#[test]
fn event_retention_days_defaults_to_thirty() {
    let from_empty: ScoringConfig = serde_json::from_str(r#"{}"#).expect("空对象应解析");
    assert_eq!(from_empty.event_retention_days, 30, "缺省保留 30 日");
    assert_eq!(
        ScoringConfig::default().event_retention_days,
        30,
        "Default 缺省与 serde 缺省必须同源同值"
    );
    assert_eq!(
        from_empty,
        ScoringConfig::default(),
        "缺省解析结果应等于 Default"
    );
}

#[test]
fn event_retention_days_zero_is_legal_gc_off_not_write_off() {
    let config: ScoringConfig = serde_json::from_str(r#"{"event_retention_days":0}"#)
        .expect("0 应合法（0=禁用 GC，不影响事件写入）");
    assert_eq!(config.event_retention_days, 0);
}
