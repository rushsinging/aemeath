use super::*;

#[test]
fn minimal_input_requires_agent_field() {
    let json = serde_json::json!({"prompt": "p", "description": "d"});
    assert!(serde_json::from_value::<AgentInput>(json).is_err());
}

#[test]
fn full_input_with_agent_ignores_framework_timeout() {
    // `timeout` 由框架在派发前摘除；旁路直接反序列化时 MUST 容忍而非报错。
    let json =
        serde_json::json!({"prompt": "p", "description": "d", "agent": "coder", "timeout": 50});
    let input: AgentInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.agent, "coder");
    assert_eq!(input.prompt, "p");
}
