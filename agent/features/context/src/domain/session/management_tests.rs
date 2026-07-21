use super::*;
use crate::domain::session::{
    AcceptedInputProjection, ActiveCompactMarker, CanonicalSession, CommittedRunSlice,
    CommittedRunStep, RunStepCursor, SnapshotState,
};
use share::message::Message;

fn session() -> CanonicalSession {
    CanonicalSession {
        id: "session".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: SessionMetadata::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Missing,
        revision: 0,
        compact: Some(ActiveCompactMarker {
            summary: "summary".to_string(),
            start_at: Some(RunStepCursor {
                run_id: "run-2".to_string(),
                step_id: "step-2".to_string(),
            }),
            source_revision: 1,
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
                    AcceptedInputProjection::new(vec![Message::user("visible preview")], "fp-2", 0),
                )],
            ),
        ],
        committed_steps: vec![],
    }
}

#[test]
fn list_entry_reads_marker_projected_messages() {
    let entry = SessionListEntry::from_canonical(&session());

    assert_eq!(entry.message_count, 1);
    assert_eq!(entry.preview.as_deref(), Some("visible preview"));
    assert_eq!(entry.summary, "visible preview");
}
