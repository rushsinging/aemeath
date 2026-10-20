use super::*;

#[test]
fn test_memory_config_default() {
    let config = MemoryConfig::default();

    assert!(config.enabled);
    assert_eq!(config.max_entries, 200);
    assert_eq!(config.similarity_threshold, 0.8);
    assert!(config.reflection.enabled);
    assert_eq!(config.event_retention_days, 30);
    assert_eq!(config.reflection.target_active_entries, 150);
}

#[test]
fn test_event_retention_days_defaults_to_30_and_deserializes_override() {
    // 缺省 30 天：事件流 / reflection-history 日切 segment 保留窗口。
    let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.event_retention_days, 30);

    // 显式覆盖。
    let json = r#"{ "event_retention_days": 7 }"#;
    let config: MemoryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.event_retention_days, 7);

    // `0` = 禁用 GC（从不删除）；NEVER 表示关闭写入。
    let zero: MemoryConfig = serde_json::from_str(r#"{ "event_retention_days": 0 }"#).unwrap();
    assert_eq!(zero.event_retention_days, 0);
}

#[test]
fn test_memory_config_deserialize_ignores_removed_session_end_summary() {
    let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert!(empty.enabled);
    assert_eq!(empty.max_entries, 200);
    assert_eq!(empty.reflection.interval_runs, 10);
    assert!(!empty.reflection.auto_apply_suggestions);
    assert_eq!(empty.reflection.target_active_entries, 150);

    let json = r#"{
            "enabled": true,
            "auto_summary_on_session_end": false
        }"#;
    let config: MemoryConfig = serde_json::from_str(json).unwrap();

    assert!(config.enabled);
    assert_eq!(config.max_entries, 200);
}

#[test]
fn test_memory_config_deserialize_custom() {
    let json = r#"{
            "enabled": false,
            "max_entries": 20,
            "similarity_threshold": 0.6,
            "reflection": {
                "enabled": false,
                "interval_runs": 5,
                "auto_apply_suggestions": true,
                "model": "test/model"
            }
        }"#;
    let config: MemoryConfig = serde_json::from_str(json).unwrap();

    assert!(!config.enabled);
    assert_eq!(config.max_entries, 20);
    assert_eq!(config.similarity_threshold, 0.6);
    assert!(!config.reflection.enabled);
    assert_eq!(config.reflection.interval_runs, 5);
    assert!(config.reflection.auto_apply_suggestions);
    assert_eq!(config.reflection.model.as_deref(), Some("test/model"));
}

#[test]
fn test_reflection_config_default() {
    let config = ReflectionConfig::default();

    assert!(config.enabled);
    assert_eq!(config.interval_runs, 10);
    assert!(!config.auto_apply_suggestions);
    assert!(config.model.is_none());
    assert_eq!(config.timeout_secs, 600);
    assert_eq!(config.target_active_entries, 150);
}

#[test]
fn test_reflection_timeout_secs_defaults_to_600_and_deserializes_override() {
    // 缺省：反思超时 600s——两步反思（去噪 + 归纳）在 ~60k chars 输入 +
    // Max reasoning 下串行执行，240s 实测在 deepseek-v4.1-flash 上必然超时。
    let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.reflection.timeout_secs, 600);

    // 显式配置覆盖。
    let json = r#"{ "reflection": { "timeout_secs": 300 } }"#;
    let config: MemoryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.reflection.timeout_secs, 300);
}

#[test]
fn reflection_reasoning_defaults_to_inherited_and_round_trips_override() {
    let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.reflection.reasoning, None);

    let configured: MemoryConfig =
        serde_json::from_str(r#"{"reflection":{"reasoning":"high"}}"#).unwrap();
    assert_eq!(
        configured.reflection.reasoning,
        Some(crate::reasoning::ReasoningLevel::High)
    );
    let encoded = serde_json::to_value(&configured).unwrap();
    assert_eq!(encoded["reflection"]["reasoning"], "high");
    assert!(
        serde_json::from_str::<MemoryConfig>(r#"{"reflection":{"reasoning":"unsupported"}}"#)
            .is_err()
    );
}

#[test]
fn reflection_max_tokens_defaults_to_inherited_and_round_trips_override() {
    let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.reflection.max_tokens, None);

    let configured: MemoryConfig =
        serde_json::from_str(r#"{"reflection":{"max_tokens":64000}}"#).unwrap();
    assert_eq!(configured.reflection.max_tokens, Some(64_000));
    let encoded = serde_json::to_value(&configured).unwrap();
    assert_eq!(encoded["reflection"]["max_tokens"], 64_000);
    assert!(serde_json::from_str::<MemoryConfig>(r#"{"reflection":{"max_tokens":-1}}"#).is_err());
}

#[test]
fn test_reflection_target_active_entries_defaults_to_150_and_deserializes_override() {
    // 缺省 150：reflect 将 active 收敛到该目标，hard_cap(max_entries=200) 留 50 缓冲。
    let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.reflection.target_active_entries, 150);

    let json = r#"{ "reflection": { "target_active_entries": 120 } }"#;
    let config: MemoryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.reflection.target_active_entries, 120);
}
