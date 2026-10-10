use super::*;
use sdk::ids::{RunStepId, ToolCallId};

fn call(name: &str, input: Value) -> ToolCall {
    ToolCall {
        id: ToolCallId::new_v7(),
        provider_id: format!("provider-{name}"),
        name: name.to_string(),
        index: 0,
        input,
    }
}

fn step() -> RunStepId {
    RunStepId::new_v7()
}

#[test]
fn same_step_repeated_identical_calls_count_once() {
    let mut fuse = ToolCallFuse::new();
    let step_id = step();
    let tool_call = call("Read", serde_json::json!({"file_path":"a.rs","limit":100}));

    assert_eq!(fuse.inspect(&step_id, &tool_call), ToolFuseDecision::Allow);
    assert_eq!(fuse.inspect(&step_id, &tool_call), ToolFuseDecision::Allow);
    assert_eq!(fuse.inspect(&step_id, &tool_call), ToolFuseDecision::Allow);
}

#[test]
fn soft_blocks_after_three_steps_with_identical_fingerprint() {
    let mut fuse = ToolCallFuse::new();
    let tool_call = call("Read", serde_json::json!({"file_path":"a.rs","limit":100}));

    assert_eq!(fuse.inspect(&step(), &tool_call), ToolFuseDecision::Allow);
    assert_eq!(fuse.inspect(&step(), &tool_call), ToolFuseDecision::Allow);
    assert!(matches!(
        fuse.inspect(&step(), &tool_call),
        ToolFuseDecision::SoftBlock { .. }
    ));
}

#[test]
fn escalates_to_fail_after_five_steps_with_identical_fingerprint() {
    let mut fuse = ToolCallFuse::new();
    let tool_call = call("Read", serde_json::json!({"file_path":"a.rs"}));

    for _ in 0..4 {
        let _ = fuse.inspect(&step(), &tool_call);
    }
    assert!(matches!(
        fuse.inspect(&step(), &tool_call),
        ToolFuseDecision::Fail { .. }
    ));
}

#[test]
fn escalates_to_fail_when_blocked_count_reaches_limit() {
    let mut fuse = ToolCallFuse::new();
    let tool_call = call("Read", serde_json::json!({"file_path":"a.rs"}));

    // consec Soft at step 3,4；第 3 次 SoftBlock 时 blocked_count≥3 → Fail
    // （即便尚未到 consec hard=5）
    assert_eq!(fuse.inspect(&step(), &tool_call), ToolFuseDecision::Allow);
    assert_eq!(fuse.inspect(&step(), &tool_call), ToolFuseDecision::Allow);
    assert!(matches!(
        fuse.inspect(&step(), &tool_call),
        ToolFuseDecision::SoftBlock { .. }
    ));
    assert!(matches!(
        fuse.inspect(&step(), &tool_call),
        ToolFuseDecision::SoftBlock { .. }
    ));
    assert!(matches!(
        fuse.inspect(&step(), &tool_call),
        ToolFuseDecision::Fail { .. }
    ));
}

#[test]
fn blocks_short_periodic_tool_call_loop_across_steps() {
    let mut fuse = ToolCallFuse::new();
    let a = call("Read", serde_json::json!({"file_path":"a.rs"}));
    let b = call("Read", serde_json::json!({"file_path":"b.rs"}));
    let c = call("Read", serde_json::json!({"file_path":"c.rs"}));

    for tool_call in [&a, &b, &c, &a, &b, &c, &a, &b] {
        assert_eq!(fuse.inspect(&step(), tool_call), ToolFuseDecision::Allow);
    }
    assert!(matches!(
        fuse.inspect(&step(), &c),
        ToolFuseDecision::SoftBlock { .. }
    ));
}

#[test]
fn normalizes_json_object_key_order_for_fingerprint_across_steps() {
    let mut fuse = ToolCallFuse::new();
    let left = call("Read", serde_json::json!({"b":2,"a":1}));
    let right = call("Read", serde_json::json!({"a":1,"b":2}));

    assert_eq!(fuse.inspect(&step(), &left), ToolFuseDecision::Allow);
    assert_eq!(fuse.inspect(&step(), &right), ToolFuseDecision::Allow);
    assert!(matches!(
        fuse.inspect(&step(), &left),
        ToolFuseDecision::SoftBlock { .. }
    ));
}
