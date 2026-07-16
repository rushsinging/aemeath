use context::domain::session::{
    CanonicalSession, CommittedStep, SessionCodec, SessionCodecError, SnapshotState,
    CURRENT_SESSION_SCHEMA_VERSION,
};
use serde_json::json;
use share::message::Message;

#[test]
fn current_envelope_round_trips_canonically() {
    let session = CanonicalSession::fixture("session-1");
    let bytes = SessionCodec::encode(&session).unwrap();
    let decoded = SessionCodec::decode(&bytes).unwrap();
    assert_eq!(decoded.session, session);
    assert!(!String::from_utf8(bytes).unwrap().contains("\"messages\""));
}

#[test]
fn legacy_messages_upgrade_to_single_normal_chat() {
    let bytes = serde_json::to_vec(&json!({
        "id": "legacy",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-02T00:00:00Z",
        "messages": [Message::user("legacy fact")],
        "metadata": {"title": "old"}
    }))
    .unwrap();
    let decoded = SessionCodec::decode(&bytes).unwrap();
    assert!(decoded.upgraded_from_legacy);
    assert_eq!(decoded.session.chats.len(), 1);
    assert_eq!(
        decoded.session.chats[0].messages[0].text_content(),
        "legacy fact"
    );
    assert_eq!(decoded.session.metadata.title.as_deref(), Some("old"));
    assert!(matches!(decoded.session.tasks, SnapshotState::Missing));
    assert!(matches!(decoded.session.workspace, SnapshotState::Missing));
}

#[test]
fn explicit_empty_snapshot_is_distinct_from_missing() {
    let mut session = CanonicalSession::fixture("empty");
    session.tasks = SnapshotState::CapturedEmpty;
    session.workspace = SnapshotState::CapturedEmpty;
    let decoded = SessionCodec::decode(&SessionCodec::encode(&session).unwrap()).unwrap();
    assert!(matches!(
        decoded.session.tasks,
        SnapshotState::CapturedEmpty
    ));
    assert!(matches!(
        decoded.session.workspace,
        SnapshotState::CapturedEmpty
    ));
}

#[test]
fn future_version_is_rejected_without_losing_original_bytes() {
    let bytes = serde_json::to_vec(&json!({
        "schema_version": CURRENT_SESSION_SCHEMA_VERSION + 1,
        "id": "future",
        "unknown": {"must": "survive"}
    }))
    .unwrap();
    assert!(matches!(
        SessionCodec::decode(&bytes),
        Err(SessionCodecError::UnsupportedFutureVersion { version, original_bytes })
            if version == CURRENT_SESSION_SCHEMA_VERSION + 1 && original_bytes == bytes
    ));
}

#[test]
fn committed_step_ledger_round_trips() {
    let mut session = CanonicalSession::fixture("ledger");
    session
        .committed_steps
        .push(CommittedStep::fixture("run", "step", "fingerprint", 2));
    let decoded = SessionCodec::decode(&SessionCodec::encode(&session).unwrap()).unwrap();
    assert_eq!(decoded.session.revision, 2);
    assert_eq!(decoded.session.committed_steps, session.committed_steps);
}
