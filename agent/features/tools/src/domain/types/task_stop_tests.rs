use super::*;

#[test]
fn snake_case_input() {
    let json = serde_json::json!({"task_id": "42"});
    let input: TaskStopInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.task_id, "42");
}

#[test]
fn legacy_camel_case_alias() {
    let json = serde_json::json!({"taskId": "42"});
    let input: TaskStopInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.task_id, "42");
}
