use super::*;

#[test]
fn minimal_input_requires_agent_field() {
    let json = serde_json::json!({"prompt": "p", "description": "d"});
    assert!(serde_json::from_value::<AgentInput>(json).is_err());
}

#[test]
fn full_input_with_agent_and_timeout() {
    let json =
        serde_json::json!({"prompt": "p", "description": "d", "agent": "coder", "timeout": 50});
    let input: AgentInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.agent, "coder");
    assert_eq!(input.timeout, Some(50));
}
