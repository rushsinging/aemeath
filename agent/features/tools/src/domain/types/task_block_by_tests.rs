use super::*;
use crate::domain::types::ToolSchema;

#[test]
fn task_block_by_schema_publishes_complete_dependency_list() {
    let schema = TaskBlockByInput::data_schema();
    let properties = schema["properties"].as_object().expect("properties");
    assert!(properties.contains_key("id"));
    assert!(properties.contains_key("block_by_ids"));
    assert_eq!(
        schema["required"],
        serde_json::json!(["id", "block_by_ids"])
    );
    assert_eq!(properties["block_by_ids"]["type"], "array");
}

#[test]
fn task_block_by_accepts_canonical_and_camel_case_inputs() {
    let canonical: TaskBlockByInput =
        serde_json::from_value(serde_json::json!({"id": "3", "block_by_ids": ["1", "2"]})).unwrap();
    assert_eq!(canonical.id, "3");
    assert_eq!(canonical.block_by_ids, ["1", "2"]);

    let alias: TaskBlockByInput =
        serde_json::from_value(serde_json::json!({"taskId": "3", "blockByIds": ["1"]})).unwrap();
    assert_eq!(alias.id, "3");
    assert_eq!(alias.block_by_ids, ["1"]);
}
