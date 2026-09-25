use super::*;
use crate::domain::session::{
    AcceptedInputRecord, ActiveCompactMarker, CanonicalSession, CommittedRunSlice,
    CommittedRunStep, RunStepCursor, SnapshotState,
};
use share::message::Message;
use share::session_types::{PersistedWorkspaceContext, ProjectIdentity};

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
        cleared_after: None,
        run_slices: vec![
            CommittedRunSlice::new(
                "run-1",
                vec![CommittedRunStep::accepted_only(
                    "step-1",
                    AcceptedInputRecord::new(vec![Message::user("hidden")], "fp-1", 0),
                )],
            ),
            CommittedRunSlice::new(
                "run-2",
                vec![CommittedRunStep::accepted_only(
                    "step-2",
                    AcceptedInputRecord::new(vec![Message::user("visible preview")], "fp-2", 0),
                )],
            ),
        ]
        .into(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    }
}

#[test]
fn list_entry_reads_marker_projected_messages() {
    let entry = SessionListEntry::from_canonical(&session());

    assert_eq!(entry.message_count, 1);
    assert_eq!(entry.preview.as_deref(), Some("visible preview"));
    assert_eq!(entry.summary, "visible preview");
}

/// 构造 `/clear` 后未再产生消息的 session：reader 加载时已按 cleared_after
/// 裁剪 run_slices，domain 收到的即空 active 历史 + clear 边界标记。
fn cleared_empty_session() -> CanonicalSession {
    CanonicalSession {
        cleared_after: Some(RunStepCursor {
            run_id: "run-1".to_string(),
            step_id: "step-1".to_string(),
        }),
        run_slices: Default::default(),
        compact: None,
        ..session()
    }
}

fn captured_workspace_session(initial_cwd: &str) -> CanonicalSession {
    let mut session = cleared_empty_session();
    session.workspace = SnapshotState::Captured(PersistedWorkspaceContext {
        project_identity: ProjectIdentity {
            initial_cwd: initial_cwd.to_string(),
            git_common_dir: None,
        },
        ..PersistedWorkspaceContext::default()
    });
    session
}

#[test]
fn list_entry_marks_cleared_empty_tail_as_empty_instead_of_unknown() {
    let entry = SessionListEntry::from_canonical(&cleared_empty_session());

    assert_eq!(entry.message_count, 0);
    assert_eq!(entry.preview, None);
    assert_eq!(entry.summary, "(empty)");
}

#[test]
fn list_entry_derives_project_name_from_workspace_identity() {
    let entry =
        SessionListEntry::from_canonical(&captured_workspace_session("/Users/dev/work/aemeath"));

    assert_eq!(entry.project.as_deref(), Some("aemeath"));
}

#[test]
fn list_entry_prefers_explicit_metadata_project_over_workspace_identity() {
    let mut session = captured_workspace_session("/Users/dev/work/aemeath");
    session.metadata.project = Some("custom-project".to_string());

    let entry = SessionListEntry::from_canonical(&session);

    assert_eq!(entry.project.as_deref(), Some("custom-project"));
}

#[test]
fn list_entry_keeps_project_none_without_workspace_snapshot() {
    let entry = SessionListEntry::from_canonical(&cleared_empty_session());

    assert_eq!(entry.project, None);
}
