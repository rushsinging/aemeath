use super::*;
use crate::domain::types::ToolSchema;

#[test]
fn minimal_input_requires_agent_field() {
    let json = serde_json::json!({"prompt": "p", "goal": "d"});
    assert!(serde_json::from_value::<AgentInput>(json).is_err());
}

#[test]
fn full_input_with_agent_and_timeout() {
    let json = serde_json::json!({"prompt": "p", "goal": "d", "agent": "coder", "timeout": 50});
    let input: AgentInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.agent, "coder");
    assert_eq!(input.goal, "d");
    assert_eq!(input.timeout, Some(50));
}

#[test]
fn agent_input_schema_exposes_goal_not_description() {
    let schema = AgentInput::data_schema();
    let properties = schema
        .get("properties")
        .and_then(|value| value.as_object())
        .expect("properties object");
    assert!(
        properties.contains_key("goal"),
        "schema MUST expose goal: {schema}"
    );
    assert!(
        !properties.contains_key("description"),
        "schema MUST NOT expose description as property name: {schema}"
    );
}

#[test]
fn agent_input_accepts_legacy_description_alias() {
    let json = serde_json::json!({
        "prompt": "do the work",
        "description": "legacy intent",
        "agent": "coder"
    });
    let input: AgentInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.goal, "legacy intent");
}
