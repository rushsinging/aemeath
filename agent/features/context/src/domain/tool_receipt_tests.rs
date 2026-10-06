use super::tool_receipt::*;
use super::{SessionId, ToolOutcomeKindData};
use sdk::{RunId, RunStepId};

fn identity() -> ToolCallIdentityData {
    ToolCallIdentityData {
        session_id: SessionId::new("session-1"),
        run_id: RunId::new("run-1"),
        step_id: RunStepId::new("step-1"),
        runtime_call_id: "runtime-call-1".to_string(),
        provider_call_id: Some("provider-call-1".to_string()),
        tool_name: "Glob".to_string(),
        call_index: 0,
        agent: false,
    }
}

#[test]
fn tool_receipt_state_is_monotonic_and_idempotent() {
    let pending = ToolCallReceiptData::pending(identity(), "pattern=**/archify.mjs");
    let running = pending
        .advance(ToolReceiptMutationData::running(identity()))
        .expect("Pending -> Running 应合法")
        .receipt;
    let terminal = ToolTerminalReceiptData::new(
        ToolOutcomeKindData::TimedOut,
        "达到 effective deadline",
        CleanupConfirmation::Confirmed,
    );
    let timed_out = running
        .advance(ToolReceiptMutationData::terminal(
            identity(),
            terminal.clone(),
        ))
        .expect("Running -> TimedOut 应合法")
        .receipt;

    let repeated = timed_out
        .advance(ToolReceiptMutationData::terminal(identity(), terminal))
        .expect("相同 terminal mutation 应幂等");
    assert!(!repeated.changed);
    assert!(matches!(repeated.receipt.state, ToolCallState::Terminal(_)));
}

#[test]
fn cancellation_unconfirmed_preserves_side_effects_and_unfinished_ids() {
    let running = ToolCallReceiptData::pending(identity(), "command=external")
        .advance(ToolReceiptMutationData::running(identity()))
        .unwrap()
        .receipt;
    let terminal = ToolTerminalReceiptData::new(
        ToolOutcomeKindData::CancellationUnconfirmed,
        "底层工作未确认停止",
        CleanupConfirmation::Unconfirmed,
    )
    .with_possible_side_effect("外部进程可能仍在运行")
    .with_unfinished_call("child-1");

    let result = running
        .advance(ToolReceiptMutationData::terminal(identity(), terminal))
        .unwrap();
    let ToolCallState::Terminal(terminal) = result.receipt.state else {
        panic!("应为 terminal");
    };
    assert_eq!(terminal.possible_side_effects(), ["外部进程可能仍在运行"]);
    assert_eq!(terminal.unfinished_call_ids(), ["child-1"]);
}

#[test]
fn terminal_receipt_rejects_state_regression_and_conflicting_terminal() {
    let terminal = ToolCallReceiptData::pending(identity(), "safe")
        .advance(ToolReceiptMutationData::terminal(
            identity(),
            ToolTerminalReceiptData::new(
                ToolOutcomeKindData::Denied,
                "审批拒绝",
                CleanupConfirmation::NotApplicable,
            ),
        ))
        .unwrap()
        .receipt;

    assert!(matches!(
        terminal
            .clone()
            .advance(ToolReceiptMutationData::running(identity())),
        Err(ToolReceiptMutationError::TerminalStateConflict { .. })
    ));
    assert!(matches!(
        terminal.advance(ToolReceiptMutationData::terminal(
            identity(),
            ToolTerminalReceiptData::new(
                ToolOutcomeKindData::Failure,
                "另一终态",
                CleanupConfirmation::NotApplicable,
            ),
        )),
        Err(ToolReceiptMutationError::TerminalStateConflict { .. })
    ));
}

#[test]
fn timed_out_is_a_distinct_tool_outcome_kind() {
    assert_ne!(ToolOutcomeKindData::TimedOut, ToolOutcomeKindData::Failure);
    assert_ne!(
        ToolOutcomeKindData::TimedOut,
        ToolOutcomeKindData::CancellationUnconfirmed
    );
}

#[test]
fn running_receipt_advances_to_backgrounded_then_terminal() {
    let running = ToolCallReceiptData::pending(identity(), "command=cargo test")
        .advance(ToolReceiptMutationData::running(identity()))
        .unwrap()
        .receipt;

    let backgrounded = running
        .advance(ToolReceiptMutationData::backgrounded(identity()))
        .expect("Running -> Backgrounded 应合法（转后台）")
        .receipt;
    assert!(matches!(backgrounded.state, ToolCallState::Backgrounded));
    assert!(
        backgrounded.to_step_receipt().is_none(),
        "Backgrounded 非终态，不应产出 step receipt"
    );

    let terminal = backgrounded
        .advance(ToolReceiptMutationData::terminal(
            identity(),
            ToolTerminalReceiptData::new(
                ToolOutcomeKindData::Success,
                "tool completed",
                CleanupConfirmation::NotApplicable,
            ),
        ))
        .expect("Backgrounded -> Terminal 应合法（后台真实完成）")
        .receipt;
    assert!(matches!(terminal.state, ToolCallState::Terminal(_)));
    assert!(terminal.to_step_receipt().is_some());
}

#[test]
fn backgrounded_mutation_is_idempotent_and_rejects_regression() {
    let backgrounded = ToolCallReceiptData::pending(identity(), "command=build")
        .advance(ToolReceiptMutationData::running(identity()))
        .unwrap()
        .receipt
        .advance(ToolReceiptMutationData::backgrounded(identity()))
        .unwrap()
        .receipt;

    let repeated = backgrounded
        .clone()
        .advance(ToolReceiptMutationData::backgrounded(identity()))
        .expect("重复 Backgrounded mutation 应幂等");
    assert!(!repeated.changed);

    assert!(matches!(
        backgrounded
            .clone()
            .advance(ToolReceiptMutationData::running(identity())),
        Err(ToolReceiptMutationError::InvalidTransition)
    ));

    let pending = ToolCallReceiptData::pending(identity(), "safe");
    assert!(matches!(
        pending.advance(ToolReceiptMutationData::backgrounded(identity())),
        Err(ToolReceiptMutationError::InvalidTransition)
    ));
}

#[test]
fn backgrounded_state_serde_round_trips() {
    let backgrounded = ToolCallReceiptData::pending(identity(), "command=build")
        .advance(ToolReceiptMutationData::running(identity()))
        .unwrap()
        .receipt
        .advance(ToolReceiptMutationData::backgrounded(identity()))
        .unwrap()
        .receipt;

    let encoded = serde_json::to_string(&backgrounded).unwrap();
    let decoded: ToolCallReceiptData = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, backgrounded);
}
