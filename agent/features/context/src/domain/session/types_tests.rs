use super::SessionMetadata;

#[test]
fn legacy_metadata_without_reasoning_effort_deserializes() {
    let raw = serde_json::json!({
        "title": "调试会话",
        "model": "zhipu/glm-5.3"
    });
    let metadata: SessionMetadata = serde_json::from_value(raw).expect("旧盘元数据应可读取");

    assert_eq!(metadata.model.as_deref(), Some("zhipu/glm-5.3"));
    assert_eq!(metadata.reasoning_effort, None);
}

#[test]
fn reasoning_effort_roundtrips_through_serialization() {
    let metadata = SessionMetadata {
        reasoning_effort: Some("xhigh".to_string()),
        ..Default::default()
    };
    let serialized = serde_json::to_value(&metadata).expect("元数据应可序列化");
    let restored: SessionMetadata = serde_json::from_value(serialized).expect("元数据应可反序列化");

    assert_eq!(restored.reasoning_effort.as_deref(), Some("xhigh"));
}
