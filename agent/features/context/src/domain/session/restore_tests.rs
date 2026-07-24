use super::*;
use crate::domain::session::{
    AcceptedInputProjection, ActiveCompactMarker, CanonicalSession, CommittedRunSlice,
    CommittedRunStep, RunStepCursor, SnapshotState,
};
use share::message::Message;

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

    assert_eq!(restore.steps.len(), 2);
    assert_eq!(restore.steps[0].run_id, "run-1");
    assert_eq!(restore.steps[0].step_id, "step-1");
    assert_eq!(restore.steps[0].messages[0].text_content(), "first");
    assert_eq!(restore.steps[1].run_id, "run-1");
    assert_eq!(restore.steps[1].step_id, "step-2");
    assert_eq!(restore.steps[1].messages[0].text_content(), "second");
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
    assert_eq!(restore.steps.len(), 1);
    assert_eq!(restore.steps[0].run_id, "run-2");
    assert_eq!(restore.steps[0].step_id, "step-2");
    assert_eq!(restore.steps[0].messages[0].text_content(), "visible");
}
