use super::*;
use crate::application::agent::ToolCall;
use crate::application::loop_engine::ToolGuardDecision;
use policy::{PolicyDecision, PolicyPort, PolicyReason, PolicyRequest};
use sdk::ids::ToolCallId;
use std::sync::Mutex;
use tools::composition::TestCatalogExecutionFactory;
use tools::{ToolExecutionContext, TypedTool, TypedToolResult};

struct RecordingPolicy {
    names: Mutex<Vec<String>>,
}

impl PolicyPort for RecordingPolicy {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        self.names
            .lock()
            .unwrap()
            .push(request.tool_name().as_str().to_string());
        if request.tool_name().as_str() == "denied" {
            PolicyDecision::Deny {
                reason: PolicyReason::RestrictedTool,
            }
        } else {
            PolicyDecision::Allow
        }
    }
}

struct TestTool(&'static str);

#[async_trait::async_trait]
impl TypedTool for TestTool {
    type Output = serde_json::Value;

    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "tool coordination test"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        TypedToolResult::success("ok", serde_json::Value::Null)
    }
}

fn call(name: &str, index: usize) -> ToolCall {
    ToolCall {
        id: ToolCallId::from_legacy_or_new(&format!("runtime-{index}")),
        provider_id: format!("provider-{index}"),
        name: name.to_string(),
        index,
        input: serde_json::json!({"index": index}),
    }
}

#[test]
fn prepare_round_applies_guard_before_policy_and_preserves_positions() {
    let factory = TestCatalogExecutionFactory::new();
    factory.register(TestTool("Allowed"));
    factory.register(TestTool("Denied"));
    let ctx = crate::application::testing::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        tokio_util::sync::CancellationToken::new(),
    );
    let catalog = factory.build(ctx).catalog();
    let policy = RecordingPolicy {
        names: Mutex::new(Vec::new()),
    };
    let calls = vec![
        (call("Allowed", 0), ToolGuardDecision::Allow),
        (
            call("Allowed", 1),
            ToolGuardDecision::SoftBlock {
                reason: "loop".to_string(),
            },
        ),
        (call("Denied", 2), ToolGuardDecision::Allow),
    ];

    let prepared = prepare_tool_round(
        &calls,
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    assert_eq!(
        policy.names.lock().unwrap().as_slice(),
        ["allowed", "denied"]
    );
    assert_eq!(prepared.executable.len(), 1);
    assert_eq!(prepared.executable[0].index, 0);
    assert_eq!(prepared.guard_blocked.len(), 1);
    assert_eq!(prepared.guard_blocked[0].call_id, calls[1].0.id);
    assert_eq!(prepared.denied.len(), 1);
    assert_eq!(prepared.denied[0].call.id, calls[2].0.id);
}

#[test]
fn restore_tool_call_order_uses_original_call_order() {
    let calls = vec![call("Allowed", 0), call("Allowed", 1), call("Allowed", 2)];
    let results = vec![
        crate::application::agent::ToolExecution::new(
            &calls[2],
            tools::ToolOutcome::new("third", serde_json::Value::Null, Vec::new()),
        ),
        crate::application::agent::ToolExecution::new(
            &calls[0],
            tools::ToolOutcome::new("first", serde_json::Value::Null, Vec::new()),
        ),
        crate::application::agent::ToolExecution::new(
            &calls[1],
            tools::ToolOutcome::new("second", serde_json::Value::Null, Vec::new()),
        ),
    ];

    let ordered = restore_tool_call_order(&calls, results);

    assert_eq!(
        ordered
            .iter()
            .map(|result| result.outcome.text.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
}
