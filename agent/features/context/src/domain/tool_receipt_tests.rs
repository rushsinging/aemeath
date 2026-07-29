use super::tool_receipt::*;
use super::{SessionId, ToolOutcomeKind};
use sdk::{RunId, RunStepId};

fn identity() -> ToolCallIdentity {
    ToolCallIdentity {
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
    let pending = ToolCallReceipt::pending(identity(), "pattern=**/archify.mjs");
    let running = pending
        .advance(ToolReceiptMutation::running(identity()))
        .expect("Pending -> Running 应合法")
        .receipt;
    let terminal = ToolTerminalReceipt::new(
        ToolOutcomeKind::TimedOut,
        "达到 effective deadline",
        CleanupConfirmation::Confirmed,
    );
    let timed_out = running
        .advance(ToolReceiptMutation::terminal(identity(), terminal.clone()))
        .expect("Running -> TimedOut 应合法")
        .receipt;

    let repeated = timed_out
        .advance(ToolReceiptMutation::terminal(identity(), terminal))
        .expect("相同 terminal mutation 应幂等");
    assert!(!repeated.changed);
    assert!(matches!(repeated.receipt.state, ToolCallState::Terminal(_)));
}

#[test]
fn cancellation_unconfirmed_preserves_side_effects_and_unfinished_ids() {
    let running = ToolCallReceipt::pending(identity(), "command=external")
        .advance(ToolReceiptMutation::running(identity()))
        .unwrap()
        .receipt;
    let terminal = ToolTerminalReceipt::new(
        ToolOutcomeKind::CancellationUnconfirmed,
        "底层工作未确认停止",
        CleanupConfirmation::Unconfirmed,
    )
    .with_possible_side_effect("外部进程可能仍在运行")
    .with_unfinished_call("child-1");

    let result = running
        .advance(ToolReceiptMutation::terminal(identity(), terminal))
        .unwrap();
    let ToolCallState::Terminal(terminal) = result.receipt.state else {
        panic!("应为 terminal");
    };
    assert_eq!(terminal.possible_side_effects(), ["外部进程可能仍在运行"]);
    assert_eq!(terminal.unfinished_call_ids(), ["child-1"]);
}

#[test]
fn terminal_receipt_rejects_state_regression_and_conflicting_terminal() {
    let terminal = ToolCallReceipt::pending(identity(), "safe")
        .advance(ToolReceiptMutation::terminal(
            identity(),
            ToolTerminalReceipt::new(
                ToolOutcomeKind::Denied,
                "审批拒绝",
                CleanupConfirmation::NotApplicable,
            ),
        ))
        .unwrap()
        .receipt;

    assert!(matches!(
        terminal
            .clone()
            .advance(ToolReceiptMutation::running(identity())),
        Err(ToolReceiptMutationError::TerminalStateConflict { .. })
    ));
    assert!(matches!(
        terminal.advance(ToolReceiptMutation::terminal(
            identity(),
            ToolTerminalReceipt::new(
                ToolOutcomeKind::Failure,
                "另一终态",
                CleanupConfirmation::NotApplicable,
            ),
        )),
        Err(ToolReceiptMutationError::TerminalStateConflict { .. })
    ));
}

#[test]
fn timed_out_is_a_distinct_tool_outcome_kind() {
    assert_ne!(ToolOutcomeKind::TimedOut, ToolOutcomeKind::Failure);
    assert_ne!(
        ToolOutcomeKind::TimedOut,
        ToolOutcomeKind::CancellationUnconfirmed
    );
}
