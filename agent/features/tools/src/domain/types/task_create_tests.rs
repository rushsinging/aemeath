use super::*;
use crate::domain::types::ToolSchema;

#[test]
fn task_create_schema_does_not_publish_legacy_fields() {
    let schema = TaskCreateInput::data_schema();
    let properties = schema["properties"]
        .as_object()
        .expect("task create schema properties");
    assert!(!properties.contains_key("owner"));
    assert!(!properties.contains_key("session_id"));
    assert!(!properties.contains_key("sessionId"));
    assert!(properties.contains_key("subject"));
    assert!(properties.contains_key("description"));
}
