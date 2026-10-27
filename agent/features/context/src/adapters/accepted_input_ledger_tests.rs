use super::AtomicBlobAcceptedInputLedger;
use crate::domain::session::{AcceptedInputRecord, CanonicalSession};
use share::message::Message;
use std::sync::Arc;

#[tokio::test]
async fn overlay_does_not_advance_canonical_session_revision() {
    let root = tempfile::tempdir().expect("temporary storage root");
    let blob: Arc<dyn storage::AtomicBlobPort> =
        storage::wire_file_system_blob(root.path()).expect("blob adapter");
    let ledger = AtomicBlobAcceptedInputLedger::new(blob, "019fa1be-bab3-7c47-ad94-c2952813dee8")
        .expect("accepted input ledger");
    ledger
        .save(
            35250,
            "run",
            "step",
            &AcceptedInputRecord::new(vec![Message::user("queued")], "queued", 35250),
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
