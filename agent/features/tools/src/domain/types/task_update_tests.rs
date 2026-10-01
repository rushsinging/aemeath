use super::*;
use crate::domain::types::ToolSchema;

#[test]
fn task_update_schema_only_advertises_supported_fields() {
    let schema = TaskUpdateInput::data_schema();
    let key_description = schema["properties"]["key"]["description"]
        .as_str()
        .expect("key description");
    let value_description = schema["properties"]["value"]["description"]
        .as_str()
        .expect("value description");
    assert!(!key_description.contains("owner"));
    assert!(!key_description.contains("blocked_by_id"));
    assert!(!value_description.contains("blocked_by_id"));
    assert!(key_description.contains("status"));
    assert!(key_description.contains("description"));
    assert!(key_description.contains("TaskBlockBy"));
    assert!(key_description.contains("dependencies"));
}
