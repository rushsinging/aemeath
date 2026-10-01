use crate::config::models::types::ModelEntryConfig;

#[test]
fn test_deserialize_reads_reasoning_effort() {
    let json = r#"{ "id": "m", "reasoning": true, "reasoning_effort": "xhigh" }"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning, Some(true));
    assert_eq!(entry.reasoning_effort.as_deref(), Some("xhigh"));
}

#[test]
fn test_deserialize_reasoning_effort_defaults_none() {
    let json = r#"{ "id": "m", "reasoning": true }"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning_effort, None);
}

/// 存量兼容：Connect 曾落盘 camelCase `reasoningEffort`，读取侧必须
/// 经 alias 归一，避免存量配置的推理档位被静默丢弃。
#[test]
fn deserialize_reads_legacy_camel_case_reasoning_effort_alias() {
    let json = r#"{ "id": "m", "reasoningEffort": "xhigh" }"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning_effort.as_deref(), Some("xhigh"));
}
