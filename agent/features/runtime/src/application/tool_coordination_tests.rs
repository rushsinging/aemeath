use super::*;
use crate::application::agent::ToolCall;
use crate::application::loop_engine::ToolGuardDecision;
use policy::{ApprovalSubject, PolicyDecision, PolicyPort, PolicyReason, PolicyRequest};
use sdk::ids::ToolCallId;
use std::sync::Mutex;
use tools::composition::TestCatalogExecutionFactory;
use tools::{ToolExecutionContext, TypedTool, TypedToolResult};

struct RecordingPolicy {
    names: Mutex<Vec<String>>,
    decision: Option<PolicyDecision>,
}

impl RecordingPolicy {
    fn allow_except_denied() -> Self {
        Self {
            names: Mutex::new(Vec::new()),
            decision: None,
        }
    }

    fn returning(decision: PolicyDecision) -> Self {
        Self {
            names: Mutex::new(Vec::new()),
            decision: Some(decision),
        }
    }
}

impl PolicyPort for RecordingPolicy {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        self.names
            .lock()
            .unwrap()
            .push(request.tool_name().as_str().to_string());
        self.decision.clone().unwrap_or_else(|| {
            if request.tool_name().as_str().eq_ignore_ascii_case("denied") {
                PolicyDecision::Deny {
                    reason: PolicyReason::RestrictedTool,
                }
            } else {
                PolicyDecision::Allow
            }
        })
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
    let policy = RecordingPolicy::allow_except_denied();
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
        ["Allowed", "Denied"]
    );
    assert_eq!(prepared.executable.len(), 1);
    assert_eq!(prepared.executable[0].index, 0);
    assert_eq!(prepared.guard_blocked.len(), 1);
    assert_eq!(prepared.guard_blocked[0].call_id, calls[1].0.id);
    assert_eq!(prepared.denied.len(), 1);
    assert_eq!(prepared.denied[0].call.id, calls[2].0.id);
}

#[test]
fn prepare_round_maps_require_approval_to_denied_call_with_subject_and_reason() {
    let factory = TestCatalogExecutionFactory::new();
    factory.register(TestTool("Bash"));
    let ctx = crate::application::testing::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        tokio_util::sync::CancellationToken::new(),
    );
    let catalog = factory.build(ctx).catalog();
    let policy = RecordingPolicy::returning(PolicyDecision::RequireApproval {
        reason: PolicyReason::RestrictedWorkspace,
        subject: ApprovalSubject::UserInteraction,
    });

    let prepared = prepare_tool_round(
        &[(call("Bash", 0), ToolGuardDecision::Allow)],
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    assert!(prepared.executable.is_empty());
    assert_eq!(prepared.denied.len(), 1);
    assert_eq!(prepared.denied[0].call.name, "Bash");
    assert_eq!(
        prepared.denied[0].reason,
        "approval required: UserInteraction: RestrictedWorkspace"
    );
}

#[test]
fn prepare_round_rejects_missing_catalog_tool_without_invoking_policy() {
    let factory = TestCatalogExecutionFactory::new();
    let ctx = crate::application::testing::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        tokio_util::sync::CancellationToken::new(),
    );
    let catalog = factory.build(ctx).catalog();
    let policy = RecordingPolicy::returning(PolicyDecision::Allow);

    let prepared = prepare_tool_round(
        &[(call("Unknown", 0), ToolGuardDecision::Allow)],
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    assert!(prepared.executable.is_empty());
    assert_eq!(prepared.denied.len(), 1);
    assert_eq!(prepared.denied[0].call.name, "Unknown");
    assert_eq!(
        prepared.denied[0].reason,
        "Tool is not present in the catalog"
    );
    assert!(policy.names.lock().unwrap().is_empty());
}

#[test]
fn prepare_round_rejects_invalid_policy_request_without_invoking_policy() {
    let factory = TestCatalogExecutionFactory::new();
    factory.register(TestTool("Read"));
    let ctx = crate::application::testing::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        tokio_util::sync::CancellationToken::new(),
    );
    let catalog = factory.build(ctx).catalog();
    let policy = RecordingPolicy::returning(PolicyDecision::Allow);

    let prepared = prepare_tool_round(
        &[(call("Read", 0), ToolGuardDecision::Allow)],
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        std::path::Path::new(""),
    );

    assert!(prepared.executable.is_empty());
    assert_eq!(prepared.denied.len(), 1);
    assert_eq!(prepared.denied[0].call.name, "Read");
    assert_eq!(prepared.denied[0].reason, "Policy 请求的工作区根不能为空");
    assert!(policy.names.lock().unwrap().is_empty());
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
