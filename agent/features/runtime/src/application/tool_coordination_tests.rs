use super::*;
use crate::application::agent::ToolCall;
use crate::application::hook_adapter::{RuntimeHookDirective, RuntimeHookReason};
use crate::application::loop_engine::ToolGuardDecision;
use policy::{ApprovalSubject, PolicyDecision, PolicyPort, PolicyReason, PolicyRequest};
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
        if request.tool_name().as_str() == "Denied" {
            PolicyDecision::Deny {
                reason: PolicyReason::RestrictedTool,
            }
        } else {
            PolicyDecision::Allow(tools::AuthorizationContext::STANDARD)
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
fn prepare_round_applies_policy_before_fuse_and_preserves_positions() {
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
        ["Allowed", "Allowed", "Denied"]
    );
    assert_eq!(prepared.executable.len(), 1);
    assert_eq!(prepared.executable[0].call.index, 0);
    assert_eq!(prepared.guard_blocked.len(), 1);
    assert_eq!(prepared.guard_blocked[0].call_id, calls[1].0.id);
    assert_eq!(prepared.denied.len(), 1);
    assert_eq!(prepared.denied[0].call.id, calls[2].0.id);
}

struct AllowAllRecordingPolicy {
    names: Mutex<Vec<String>>,
}

impl PolicyPort for AllowAllRecordingPolicy {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        self.names
            .lock()
            .unwrap()
            .push(request.tool_name().as_str().to_string());
        PolicyDecision::Allow(tools::AuthorizationContext::ALLOW_ALL)
    }
}

#[test]
fn allow_all_bypasses_fuse_after_single_policy_evaluation() {
    let factory = TestCatalogExecutionFactory::new();
    factory.register(TestTool("Allowed"));
    let ctx = crate::application::testing::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        tokio_util::sync::CancellationToken::new(),
    );
    let catalog = factory.build(ctx).catalog();
    let policy = AllowAllRecordingPolicy {
        names: Mutex::new(Vec::new()),
    };
    let call = call("Allowed", 0);
    let prepared = prepare_tool_round(
        &[(
            call.clone(),
            ToolGuardDecision::SoftBlock {
                reason: "loop".to_string(),
            },
        )],
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    assert_eq!(policy.names.lock().unwrap().as_slice(), ["Allowed"]);
    assert_eq!(prepared.executable.len(), 1);
    assert!(prepared.guard_blocked.is_empty());
    assert_eq!(prepared.fuse_bypassed, vec![call.id]);
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

// ─── apply_hook_directive_to_tool_call tests ──────────────────

/// Policy that records every evaluation and returns configurable decisions
/// based on the tool name.
struct HookTestPolicy {
    eval_count: Mutex<usize>,
    authorization: tools::AuthorizationContext,
}

impl HookTestPolicy {
    fn new() -> Self {
        Self::with_authorization(tools::AuthorizationContext::STANDARD)
    }

    fn with_authorization(authorization: tools::AuthorizationContext) -> Self {
        Self {
            eval_count: Mutex::new(0),
            authorization,
        }
    }

    fn eval_count(&self) -> usize {
        *self.eval_count.lock().unwrap()
    }
}

impl PolicyPort for HookTestPolicy {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        *self.eval_count.lock().unwrap() += 1;
        match request.tool_name().as_str() {
            "Denied" => PolicyDecision::Deny {
                reason: PolicyReason::RestrictedTool,
            },
            "ApprovalRequired" => PolicyDecision::RequireApproval {
                reason: PolicyReason::RestrictedTool,
                subject: ApprovalSubject::UserInteraction,
            },
            _ => PolicyDecision::Allow(self.authorization),
        }
    }
}

/// A tool with a strict schema requiring a `path` string field, used to test
/// `validate_tool_input` failures.
struct StrictTool;

#[async_trait::async_trait]
impl TypedTool for StrictTool {
    type Output = serde_json::Value;

    fn name(&self) -> &str {
        "Strict"
    }

    fn description(&self) -> &str {
        "strict schema test tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        TypedToolResult::success("ok", serde_json::Value::Null)
    }
}

fn build_catalog() -> tools::ToolCatalogSnapshot {
    let factory = TestCatalogExecutionFactory::new();
    factory.register(TestTool("Allowed"));
    factory.register(TestTool("Denied"));
    factory.register(TestTool("ApprovalRequired"));
    factory.register(StrictTool);
    let ctx = crate::application::testing::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        tokio_util::sync::CancellationToken::new(),
    );
    factory.build(ctx).catalog()
}

#[test]
fn hook_directive_continue_returns_continue_without_context() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("Allowed", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::Continue,
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::Continue { call, context } => {
            assert_eq!(call.index, original.index);
            assert_eq!(call.input, original.input);
            assert!(context.is_none());
        }
        other => panic!("expected Continue, got {other:?}"),
    }
    // Continue must NOT touch policy.
    assert_eq!(policy.eval_count(), 0);
}

#[test]
fn hook_directive_context_preserves_context_in_continue() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("Allowed", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::Context {
            context: "extra guidance".to_string(),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::Continue { call, context } => {
            assert_eq!(call.index, original.index);
            assert_eq!(context.as_deref(), Some("extra guidance"));
        }
        other => panic!("expected Continue, got {other:?}"),
    }
    assert_eq!(policy.eval_count(), 0);
}

#[test]
fn hook_directive_block_preserves_structured_reason() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("Allowed", 0);
    let reason = RuntimeHookReason::JsonBlock {
        reason: "hook says no".to_string(),
    };

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::Block {
            reason: reason.clone(),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::Blocked {
            call,
            reason: actual,
        } => {
            assert_eq!(call.index, original.index);
            assert_eq!(actual, reason);
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
    assert_eq!(policy.eval_count(), 0);
}

#[test]
fn hook_directive_updated_input_valid_passes_schema_and_policy() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("Allowed", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::UpdatedInput {
            input: serde_json::json!({"path": "/tmp/file"}),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::Ready { call, context, .. } => {
            assert_eq!(call.name, "Allowed");
            assert_eq!(call.input, serde_json::json!({"path": "/tmp/file"}));
            assert!(context.is_none());
        }
        other => panic!("expected Ready, got {other:?}"),
    }
    // UpdatedInput must re-evaluate policy exactly once.
    assert_eq!(policy.eval_count(), 1);
}

#[test]
fn hook_directive_updated_input_preserves_policy_authorization() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::with_authorization(tools::AuthorizationContext::ALLOW_ALL);
    let original = call("Allowed", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::UpdatedInput {
            input: serde_json::json!({"path": "/tmp/file"}),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::Ready { authorization, .. } => {
            assert_eq!(authorization, tools::AuthorizationContext::ALLOW_ALL);
        }
        other => panic!("expected Ready, got {other:?}"),
    }
    assert_eq!(policy.eval_count(), 1);
}

#[test]
fn hook_directive_updated_input_invalid_returns_invalid_input_with_original_call() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("Strict", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::UpdatedInput {
            input: serde_json::json!({"wrong_field": "x"}),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::InvalidInput { call, error } => {
            // Original call preserved — updated input discarded.
            assert_eq!(call.index, original.index);
            assert_eq!(call.input, original.input);
            assert!(error.contains("path"));
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
    // Schema failure must short-circuit before policy.
    assert_eq!(policy.eval_count(), 0);
}

#[test]
fn hook_directive_updated_input_policy_denied_returns_denied() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("Denied", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::UpdatedInput {
            input: serde_json::json!({"index": 99}),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::Denied { call, reason } => {
            assert_eq!(call.name, "Denied");
            assert!(reason.contains("RestrictedTool"));
        }
        other => panic!("expected Denied, got {other:?}"),
    }
    assert_eq!(policy.eval_count(), 1);
}

#[test]
fn hook_directive_updated_input_policy_approval_required_returns_approval_required() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("ApprovalRequired", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::UpdatedInput {
            input: serde_json::json!({"index": 42}),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::ApprovalRequired { call, reason } => {
            assert_eq!(call.name, "ApprovalRequired");
            assert_eq!(call.input, serde_json::json!({"index": 42}));
            assert!(reason.contains("approval required"));
        }
        other => panic!("expected ApprovalRequired, got {other:?}"),
    }
    assert_eq!(policy.eval_count(), 1);
}

#[test]
fn hook_directive_context_and_input_preserves_context_in_ready() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("Allowed", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::ContextAndInput {
            context: "important context".to_string(),
            input: serde_json::json!({"path": "/updated"}),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::Ready { call, context, .. } => {
            assert_eq!(call.input, serde_json::json!({"path": "/updated"}));
            assert_eq!(context.as_deref(), Some("important context"));
        }
        other => panic!("expected Ready, got {other:?}"),
    }
    assert_eq!(policy.eval_count(), 1);
}

#[test]
fn hook_directive_updated_input_tool_not_in_catalog_returns_denied() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("GhostTool", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::UpdatedInput {
            input: serde_json::json!({"any": "thing"}),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    match outcome {
        HookDirectiveOutcome::Denied { call, reason } => {
            assert_eq!(call.name, "GhostTool");
            assert!(reason.contains("catalog"));
        }
        other => panic!("expected Denied, got {other:?}"),
    }
    // Catalog miss short-circuits before policy.
    assert_eq!(policy.eval_count(), 0);
}

#[test]
fn hook_directive_context_and_input_invalid_schema_returns_invalid_input() {
    let catalog = build_catalog();
    let policy = HookTestPolicy::new();
    let original = call("Strict", 0);

    let outcome = apply_hook_directive_to_tool_call(
        &original,
        RuntimeHookDirective::ContextAndInput {
            context: "ctx".to_string(),
            input: serde_json::json!({"missing": "required"}),
        },
        &catalog,
        &policy,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &std::env::current_dir().unwrap(),
    );

    assert!(matches!(outcome, HookDirectiveOutcome::InvalidInput { .. }));
    assert_eq!(policy.eval_count(), 0);
}
