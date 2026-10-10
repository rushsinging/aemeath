use super::*;

#[test]
fn test_memory_config_default() {
    let config = MemoryConfig::default();

    assert!(config.enabled);
    assert_eq!(config.max_entries, 100);
    assert_eq!(config.similarity_threshold, 0.8);
    assert!(config.reflection.enabled);
}

#[test]
fn test_memory_config_deserialize_ignores_removed_session_end_summary() {
    let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert!(empty.enabled);
    assert_eq!(empty.max_entries, 100);
    assert_eq!(empty.reflection.interval_runs, 10);
    assert!(!empty.reflection.auto_apply_suggestions);

    let json = r#"{
            "enabled": true,
            "auto_summary_on_session_end": false
        }"#;
    let config: MemoryConfig = serde_json::from_str(json).unwrap();

    assert!(config.enabled);
    assert_eq!(config.max_entries, 100);
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
    assert_eq!(config.timeout_secs, 240);
}

#[test]
fn test_reflection_timeout_secs_defaults_to_240_and_deserializes_override() {
    // 缺省：反思超时 240s——覆盖成功耗时长尾（此前硬编码 120s 紧贴 p99）。
    let empty: MemoryConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(empty.reflection.timeout_secs, 240);

    // 显式配置覆盖。
    let json = r#"{ "reflection": { "timeout_secs": 600 } }"#;
    let config: MemoryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.reflection.timeout_secs, 600);
}
