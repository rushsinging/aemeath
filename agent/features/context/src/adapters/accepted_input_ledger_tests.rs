use super::AtomicBlobAcceptedInputLedger;
use crate::domain::session::{AcceptedInputProjection, CanonicalSession};
use share::message::Message;
use std::sync::Arc;

#[tokio::test]
async fn overlay_does_not_advance_canonical_session_revision() {
    let root = tempfile::tempdir().expect("temporary storage root");
    let blob: Arc<dyn storage::api::AtomicBlobPort> =
        Arc::new(storage::FileSystemBlobAdapter::new(root.path()).expect("blob adapter"));
    let ledger = AtomicBlobAcceptedInputLedger::new(blob, "019fa1be-bab3-7c47-ad94-c2952813dee8")
        .expect("accepted input ledger");
    ledger
        .save(
            35250,
            "run",
            "step",
            &AcceptedInputProjection::new(vec![Message::user("queued")], "queued", 35250),
        )
        .await
        .expect("save accepted input");

    let mut session = CanonicalSession::fixture("019fa1be-bab3-7c47-ad94-c2952813dee8");
    session.revision = 35244;
    ledger
        .overlay(&mut session)
        .await
        .expect("overlay accepted input");

    assert_eq!(session.revision, 35244);
    assert_eq!(session.run_slices.step_receipts("run", "step").len(), 0);
}

#[tokio::test]
async fn finalized_acknowledgement_removes_only_matching_input() {
    let root = tempfile::tempdir().expect("temporary storage root");
    let blob: Arc<dyn storage::api::AtomicBlobPort> =
        Arc::new(storage::FileSystemBlobAdapter::new(root.path()).expect("blob adapter"));
    let ledger =
        AtomicBlobAcceptedInputLedger::new(blob, "session").expect("accepted input ledger");
    for (revision, run_id, step_id) in [
        (35245, "run-a", "step-a"),
        (35246, "run-b", "step-b"),
        (35250, "run-c", "step-c"),
    ] {
        ledger
            .save(
                revision,
                run_id,
                step_id,
                &AcceptedInputProjection::new(vec![Message::user(step_id)], step_id, revision),
            )
            .await
            .expect("save accepted input");
    }

    ledger
        .acknowledge_finalized_input("run-b", "step-b")
        .await
        .expect("acknowledge finalized input");
    let mut session = CanonicalSession::fixture("session");
    session.revision = 40_000;
    ledger.overlay(&mut session).await.expect("overlay ledger");

    assert!(session.accepted_input("run-a", "step-a").is_some());
    assert!(session.accepted_input("run-b", "step-b").is_none());
    assert!(session.accepted_input("run-c", "step-c").is_some());
    assert_eq!(session.revision, 40_000);
}
