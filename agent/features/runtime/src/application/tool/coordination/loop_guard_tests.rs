use super::*;
use sdk::ids::ToolCallId;

fn call(name: &str, input: Value) -> ToolCall {
    ToolCall {
        id: ToolCallId::new_v7(),
        provider_id: format!("provider-{name}"),
        name: name.to_string(),
        index: 0,
        input,
    }
}

#[test]
fn blocks_consecutive_identical_tool_calls() {
    let mut fuse = ToolCallFuse::new();
    let tool_call = call("Read", serde_json::json!({"file_path":"a.rs","limit":100}));

    assert_eq!(fuse.inspect(&tool_call), ToolFuseDecision::Allow);
    assert_eq!(fuse.inspect(&tool_call), ToolFuseDecision::Allow);
    assert!(matches!(
        fuse.inspect(&tool_call),
        ToolFuseDecision::SoftBlock { .. }
    ));
}

#[test]
fn escalates_consecutive_identical_tool_calls_to_hard_pause() {
    let mut fuse = ToolCallFuse::new();
    let tool_call = call("Read", serde_json::json!({"file_path":"a.rs"}));

    for _ in 0..4 {
        let _ = fuse.inspect(&tool_call);
    }
    assert!(matches!(
        fuse.inspect(&tool_call),
        ToolFuseDecision::HardPause { .. }
    ));
}

#[test]
fn blocks_short_periodic_tool_call_loop() {
    let mut fuse = ToolCallFuse::new();
    let a = call("Read", serde_json::json!({"file_path":"a.rs"}));
    let b = call("Read", serde_json::json!({"file_path":"b.rs"}));
    let c = call("Read", serde_json::json!({"file_path":"c.rs"}));

    for tool_call in [&a, &b, &c, &a, &b, &c, &a, &b] {
        assert_eq!(fuse.inspect(tool_call), ToolFuseDecision::Allow);
    }
    assert!(matches!(
        fuse.inspect(&c),
        ToolFuseDecision::SoftBlock { .. }
    ));
}

#[test]
fn normalizes_json_object_key_order_for_fingerprint() {
    let mut fuse = ToolCallFuse::new();
    let left = call("Read", serde_json::json!({"b":2,"a":1}));
    let right = call("Read", serde_json::json!({"a":1,"b":2}));

    assert_eq!(fuse.inspect(&left), ToolFuseDecision::Allow);
    assert_eq!(fuse.inspect(&right), ToolFuseDecision::Allow);
    assert!(matches!(
        fuse.inspect(&left),
        ToolFuseDecision::SoftBlock { .. }
    ));
}
