use context::adapters::InMemorySessionRepository;
use context::domain::{
    ContentFingerprint, ContextAppend, ContextAppendError, ContextRequestId, FinalizeCause,
    RunStepId, SessionId, SessionRevision,
};
use context::ports::SessionRepository;
use sdk::RunId;
use share::message::Message;

fn append(fingerprint: &str) -> ContextAppend {
    ContextAppend {
        session_id: SessionId::new("session"),
        expected_revision: SessionRevision::new(0),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: ContextRequestId::new("request"),
        finalize_cause: FinalizeCause::Completed,
        messages: vec![Message::user("fact")],
        receipts: vec![],
        api_input_tokens: None,
        fingerprint: ContentFingerprint::new(fingerprint),
    }
}

#[tokio::test]
async fn same_step_and_fingerprint_is_idempotent() {
    let backing = InMemorySessionRepository::new();
    backing.seed(
        &SessionId::new("session"),
        SessionRevision::new(0),
        vec![],
        None,
    );
    let first = backing.append_finalized(&append("same")).await.unwrap();
    let second = backing.append_finalized(&append("same")).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(
        backing
            .snapshot(&SessionId::new("session"))
            .await
            .unwrap()
            .messages
            .len(),
        1
    );
}

#[tokio::test]
async fn same_step_and_different_fingerprint_conflicts() {
    let backing = InMemorySessionRepository::new();
    backing.seed(
        &SessionId::new("session"),
        SessionRevision::new(0),
        vec![],
        None,
    );
    backing.append_finalized(&append("first")).await.unwrap();
    assert!(matches!(
        backing.append_finalized(&append("other")).await,
        Err(ContextAppendError::ContentConflict { .. })
    ));
}
