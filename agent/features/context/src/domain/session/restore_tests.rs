use super::*;
use crate::domain::session::{
    AcceptedInputProjection, ActiveCompactMarker, CanonicalSession, CommittedRunSlice,
    CommittedRunStep, RunStepCursor, SnapshotState,
};
use share::message::{ContentBlock, Message, Role};

fn unfinished_tool_session(state: crate::domain::ToolCallState) -> CanonicalSession {
    let identity = crate::domain::ToolCallIdentity {
        session_id: crate::domain::SessionId::new("session"),
        run_id: sdk::RunId::new("run-1"),
        step_id: sdk::RunStepId::new("step-1"),
        runtime_call_id: "runtime-call-1".to_string(),
        provider_call_id: Some("provider-call-1".to_string()),
        tool_name: "Bash".to_string(),
        call_index: 0,
        agent: false,
    };
    let mut receipt = crate::domain::ToolCallReceipt::pending(identity.clone(), "sleep 180");
    if state == crate::domain::ToolCallState::Running {
        receipt = receipt
            .advance(crate::domain::ToolReceiptMutation::running(identity))
            .unwrap()
            .receipt;
    }
    CanonicalSession {
        id: "session".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Missing,
        revision: 0,
        compact: None,
        run_slices: vec![CommittedRunSlice::new(
            "run-1",
            vec![CommittedRunStep {
                step_id: "step-1".to_string(),
                accepted_input: None,
                outcome: Some(crate::domain::session::FinalizedOutcomeProjection {
                    finalize_cause: crate::domain::FinalizeCause::UserCancelledStep,
                    messages: vec![Message {
                        role: Role::Assistant,
                        content: vec![ContentBlock::ToolUse {
                            id: "provider-call-1".to_string(),
                            name: "Bash".to_string(),
                            input: serde_json::json!({"command": "sleep 180"}),
                        }],
                        metadata: None,
                    }],
                    receipts: Vec::new(),
                    api_input_tokens: None,
                    fingerprint: "fp".to_string(),
                    committed_revision: 1,
                }),
                tool_receipts: vec![receipt],
            }],
        )],
        committed_steps: vec![],
    }
}

fn two_step_session() -> CanonicalSession {
    CanonicalSession {
        id: "session".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Missing,
        revision: 0,
        compact: None,
        run_slices: vec![CommittedRunSlice::new(
            "run-1",
            vec![
                CommittedRunStep::accepted_only(
                    "step-1",
                    AcceptedInputProjection::new(vec![Message::user("first")], "fp-1", 0),
                ),
                CommittedRunStep::accepted_only(
                    "step-2",
                    AcceptedInputProjection::new(vec![Message::user("second")], "fp-2", 0),
                ),
            ],
        )],
        committed_steps: vec![],
    }
}

#[test]
fn restore_preserves_run_step_boundaries_for_display_projection() {
    let restore = SessionRestore::from_canonical(&two_step_session());

    assert_eq!(restore.display_steps.len(), 2);
    assert_eq!(restore.display_steps[0].run_id, "run-1");
    assert_eq!(restore.display_steps[0].step_id, "step-1");
    assert_eq!(restore.display_steps[0].messages[0].text_content(), "first");
    assert_eq!(restore.display_steps[1].run_id, "run-1");
    assert_eq!(restore.display_steps[1].step_id, "step-2");
    assert_eq!(
        restore.display_steps[1].messages[0].text_content(),
        "second"
    );
}

#[test]
fn restore_projects_unfinished_tool_receipts_as_unconfirmed_results() {
    for state in [
        crate::domain::ToolCallState::Pending,
        crate::domain::ToolCallState::Running,
    ] {
        let restore = SessionRestore::from_canonical(&unfinished_tool_session(state));

        assert_eq!(restore.trimmed, 0);
        assert_eq!(restore.display_steps.len(), 1);
        assert_eq!(restore.display_steps[0].messages.len(), 2);
        let result = &restore.display_steps[0].messages[1];
        assert_eq!(result.role, Role::User);
        let [ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        }] = result.content.as_slice()
        else {
            panic!("未完成 receipt 应恢复为 provider-safe tool_result");
        };
        assert_eq!(tool_use_id, "provider-call-1");
        assert!(*is_error);
        assert!(content.to_string().contains("CancellationUnconfirmed"));
    }
}

#[test]
fn restore_reads_only_steps_from_active_marker() {
    let session = CanonicalSession {
        id: "session".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Missing,
        revision: 0,
        compact: Some(ActiveCompactMarker {
            summary: "summary".to_string(),
            start_at: Some(RunStepCursor {
                run_id: "run-2".to_string(),
                step_id: "step-2".to_string(),
            }),
            source_revision: 0,
        }),
        run_slices: vec![
            CommittedRunSlice::new(
                "run-1",
                vec![CommittedRunStep::accepted_only(
                    "step-1",
                    AcceptedInputProjection::new(vec![Message::user("hidden")], "fp-1", 0),
                )],
            ),
            CommittedRunSlice::new(
                "run-2",
                vec![CommittedRunStep::accepted_only(
                    "step-2",
                    AcceptedInputProjection::new(vec![Message::user("visible")], "fp-2", 0),
                )],
            ),
        ],
        committed_steps: vec![],
    };

    let restore = SessionRestore::from_canonical(&session);

    assert_eq!(restore.active_messages.len(), 1);
    assert_eq!(restore.active_messages[0].text_content(), "visible");
    assert_eq!(restore.display_steps.len(), 2);
    assert_eq!(restore.display_steps[0].run_id, "run-1");
    assert_eq!(restore.display_steps[0].step_id, "step-1");
    assert_eq!(
        restore.display_steps[0].messages[0].text_content(),
        "hidden"
    );
    assert_eq!(restore.display_steps[1].run_id, "run-2");
    assert_eq!(restore.display_steps[1].step_id, "step-2");
    assert_eq!(
        restore.display_steps[1].messages[0].text_content(),
        "visible"
    );
}
