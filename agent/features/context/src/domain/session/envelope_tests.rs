use super::{
    AcceptedInputProjection, CommittedRunSlice, CommittedRunStep, CommittedStep,
    CommittedStepLedger, FinalizedOutcomeProjection, SessionHistory,
};
use crate::domain::{FinalizeCause, SessionId, ToolCallIdentity, ToolReceiptMutation};
use sdk::{RunId, RunStepId};
use share::message::Message;
use std::sync::Arc;

fn tool_identity(run_id: &str, step_id: &str) -> ToolCallIdentity {
    ToolCallIdentity {
        session_id: SessionId::new("session"),
        run_id: RunId::new(run_id),
        step_id: RunStepId::new(step_id),
        runtime_call_id: "call".to_string(),
        provider_call_id: Some("provider-call".to_string()),
        tool_name: "Glob".to_string(),
        call_index: 0,
        agent: false,
    }
}

fn finalized_outcome(text: &str) -> FinalizedOutcomeProjection {
    FinalizedOutcomeProjection {
        finalize_cause: FinalizeCause::Completed,
        duration_ms: Some(3),
        messages: vec![Message::user(text)].into(),
        receipts: Vec::new(),
        api_input_tokens: Some(5),
        fingerprint: text.to_string(),
        committed_revision: 2,
    }
}

#[test]
fn appending_committed_step_reuses_existing_ledger_entry_backing() {
    let ledger = CommittedStepLedger::from_steps(vec![CommittedStep::fixture(
        "run-existing",
        "step-existing",
        "existing",
        1,
    )]);
    let existing_entry = Arc::clone(&ledger.entries()[0]);

    let updated = ledger.append(CommittedStep::fixture("run-new", "step-new", "new", 2));

    assert!(Arc::ptr_eq(&existing_entry, &updated.entries()[0]));
    assert_eq!(updated.len(), 2);
    assert_eq!(updated.entries()[1].step_id, "step-new");
}

#[test]
fn clearing_committed_step_ledger_does_not_mutate_existing_ledger() {
    let ledger = CommittedStepLedger::from_steps(vec![CommittedStep::fixture(
        "run",
        "step",
        "fingerprint",
        1,
    )]);

    let cleared = ledger.cleared();

    assert!(cleared.is_empty());
    assert_eq!(ledger.len(), 1);
}

#[test]
fn committed_step_ledger_preserves_json_array_wire() {
    let ledger = CommittedStepLedger::from_steps(vec![CommittedStep::fixture(
        "run",
        "step",
        "fingerprint",
        2,
    )]);

    let value = serde_json::to_value(&ledger).expect("serialize ledger");

    assert_eq!(
        value,
        serde_json::json!([{
            "run_id": "run",
            "step_id": "step",
            "fingerprint": "fingerprint",
            "committed_revision": 2
        }])
    );
}

#[test]
fn appending_finalized_outcome_reuses_other_run_slice() {
    let history = SessionHistory::from_slices(vec![
        CommittedRunSlice::new(
            "run-existing",
            vec![CommittedRunStep::accepted_only(
                "step-existing",
                AcceptedInputProjection::new(vec![Message::user("existing")], "existing", 1),
            )],
        ),
        CommittedRunSlice::new(
            "run-other",
            vec![CommittedRunStep::accepted_only(
                "step-other",
                AcceptedInputProjection::new(vec![Message::user("other")], "other", 1),
            )],
        ),
    ]);
    let other_slice_pointer = Arc::as_ptr(&history.slices()[1]);

    let updated = history.append_finalized_outcome(
        "run-existing",
        "step-existing",
        finalized_outcome("done"),
    );

    assert_eq!(
        updated.slices()[0].steps[0]
            .outcome
            .as_ref()
            .expect("outcome")
            .fingerprint,
        "done"
    );
    assert_eq!(other_slice_pointer, Arc::as_ptr(&updated.slices()[1]));
}

#[test]
fn advancing_tool_receipt_reuses_other_run_slice_and_returns_changed_receipt() {
    let history = SessionHistory::from_slices(vec![CommittedRunSlice::new(
        "run-other",
        vec![CommittedRunStep::accepted_only(
            "step-other",
            AcceptedInputProjection::new(vec![Message::user("other")], "other", 1),
        )],
    )]);
    let other_slice_pointer = Arc::as_ptr(&history.slices()[0]);
    let mutation = ToolReceiptMutation::pending(tool_identity("run-new", "step-new"), "pattern");

    let (updated, advanced) = history
        .advance_tool_receipt(mutation.clone())
        .expect("advance receipt");

    assert!(advanced.changed);
    assert_eq!(other_slice_pointer, Arc::as_ptr(&updated.slices()[0]));
    assert_eq!(
        updated
            .tool_receipt(&mutation)
            .expect("stored receipt")
            .identity,
        mutation.identity
    );
}

#[test]
fn repeated_tool_receipt_is_idempotent_without_replacing_history() {
    let mutation = ToolReceiptMutation::pending(tool_identity("run", "step"), "pattern");
    let (history, first) = SessionHistory::default()
        .advance_tool_receipt(mutation.clone())
        .expect("first receipt");
    let slice_pointer = Arc::as_ptr(&history.slices()[0]);

    let (unchanged, second) = history
        .advance_tool_receipt(mutation)
        .expect("repeated receipt");

    assert!(first.changed);
    assert!(!second.changed);
    assert_eq!(slice_pointer, Arc::as_ptr(&unchanged.slices()[0]));
}

#[test]
fn clearing_history_returns_empty_without_mutating_existing_history() {
    let history = SessionHistory::from_slices(vec![CommittedRunSlice::new(
        "run",
        vec![CommittedRunStep::accepted_only(
            "step",
            AcceptedInputProjection::new(vec![Message::user("existing")], "existing", 1),
        )],
    )]);

    let cleared = history.cleared();

    assert!(cleared.slices().is_empty());
    assert_eq!(history.slices().len(), 1);
}

#[test]
fn adding_a_step_reuses_unchanged_session_history_backing() {
    let existing_slice = CommittedRunSlice::new(
        "run-existing",
        vec![CommittedRunStep::accepted_only(
            "step-existing",
            AcceptedInputProjection::new(vec![Message::user("existing")], "existing", 1),
        )],
    );
    let history = SessionHistory::from_slices(vec![existing_slice]);
    let existing_slice_pointer = Arc::as_ptr(&history.slices()[0]);

    let updated = history.append_accepted_input(
        "run-new",
        "step-new",
        AcceptedInputProjection::new(vec![Message::user("new")], "new", 2),
    );

    assert_eq!(existing_slice_pointer, Arc::as_ptr(&updated.slices()[0]));
    assert_eq!(updated.slices().len(), 2);
    assert_eq!(updated.slices()[0].run_id, "run-existing");
}

#[test]
fn updating_an_existing_step_only_replaces_its_own_slice() {
    let history = SessionHistory::from_slices(vec![
        CommittedRunSlice::new(
            "run-existing",
            vec![CommittedRunStep::accepted_only(
                "step-existing",
                AcceptedInputProjection::new(vec![Message::user("existing")], "existing", 1),
            )],
        ),
        CommittedRunSlice::new(
            "run-other",
            vec![CommittedRunStep::accepted_only(
                "step-other",
                AcceptedInputProjection::new(vec![Message::user("other")], "other", 1),
            )],
        ),
    ]);
    let other_slice_pointer = Arc::as_ptr(&history.slices()[1]);

    let updated = history.append_accepted_input(
        "run-existing",
        "step-existing",
        AcceptedInputProjection::new(vec![Message::user("replacement")], "replacement", 2),
    );

    assert_eq!(
        updated.slices()[0].steps[0]
            .accepted_input
            .as_ref()
            .unwrap()
            .fingerprint,
        "replacement"
    );
    assert_eq!(other_slice_pointer, Arc::as_ptr(&updated.slices()[1]));
}
