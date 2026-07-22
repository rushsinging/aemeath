use context::adapters::InMemorySessionRepository;
use context::domain::{
    AcceptedInputAppend, AcceptedInputError, ContentFingerprint, ContextAppend, ContextAppendError,
    ContextRequestId, FinalizeCause, RunStepId, SessionId, SessionRevision,
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

fn accepted_input(fingerprint: &str) -> AcceptedInputAppend {
    AcceptedInputAppend {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: ContextRequestId::new("request"),
        messages: vec![Message::user("accepted")],
        fingerprint: ContentFingerprint::new(fingerprint),
    }
}

#[tokio::test]
async fn accepted_input_has_independent_idempotency_from_finalized_outcome() {
    let backing = InMemorySessionRepository::new();
    let session_id = SessionId::new("session");
    backing.seed(&session_id, SessionRevision::new(0), vec![], None);

    let input = accepted_input("input-v1");
    let receipt = backing.append_accepted_input(&input).await.unwrap();
    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    assert_eq!(
        backing.append_accepted_input(&input).await.unwrap(),
        receipt
    );

    let mut outcome = append("outcome-v1");
    outcome.expected_revision = SessionRevision::new(1);
    backing.append_finalized(&outcome).await.unwrap();
    assert_eq!(
        backing.snapshot(&session_id).await.unwrap().messages.len(),
        2
    );

    let mut conflict = input;
    conflict.fingerprint = ContentFingerprint::new("input-v2");
    assert!(matches!(
        backing.append_accepted_input(&conflict).await,
        Err(AcceptedInputError::ContentConflict { .. })
    ));
}

#[tokio::test]
async fn finalized_outcome_keeps_receipt_metadata_for_idempotent_retry() {
    let backing = InMemorySessionRepository::new();
    let session_id = SessionId::new("session");
    backing.seed(&session_id, SessionRevision::new(0), vec![], None);
    let mut outcome = append("outcome-v1");
    outcome.finalize_cause = FinalizeCause::RunTerminated;
    outcome.api_input_tokens = Some(21);
    outcome.receipts = vec![context::domain::StepReceipt::agent(
        "agent-call",
        0,
        context::domain::ToolOutcomeKind::CancellationUnconfirmed,
    )];

    let first = backing.append_finalized(&outcome).await.unwrap();
    let second = backing.append_finalized(&outcome).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(first.committed_revision, SessionRevision::new(1));
    assert_eq!(
        backing.snapshot(&session_id).await.unwrap().messages[0].text_content(),
        "fact"
    );
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
