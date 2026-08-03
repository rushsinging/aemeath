use super::AtomicBlobToolReceiptLedger;
use crate::domain::session::CanonicalSession;
use crate::domain::{SessionId, ToolCallIdentity, ToolCallReceipt};
use sdk::{RunId, RunStepId};
use std::sync::Arc;

#[tokio::test]
async fn overlay_does_not_advance_canonical_session_revision() {
    let root = tempfile::tempdir().expect("temporary storage root");
    let blob: Arc<dyn storage::api::AtomicBlobPort> =
        Arc::new(storage::FileSystemBlobAdapter::new(root.path()).expect("blob adapter"));
    let ledger = AtomicBlobToolReceiptLedger::new(blob, "session").expect("tool receipt ledger");
    let identity = ToolCallIdentity {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        runtime_call_id: "call".to_string(),
        provider_call_id: None,
        tool_name: "Glob".to_string(),
        call_index: 0,
        agent: false,
    };
    ledger
        .save(4, &ToolCallReceipt::pending(identity, "preview"))
        .await
        .expect("save tool receipt");

    let mut session = CanonicalSession::fixture("session");
    session.revision = 4;
    ledger
        .overlay(&mut session)
        .await
        .expect("overlay tool receipt");

    assert_eq!(session.revision, 4);
    assert_eq!(
        session
            .run_slices
            .iter()
            .flat_map(|slice| slice.steps.iter())
            .map(|step| step.tool_receipts.len())
            .sum::<usize>(),
        1
    );
}
