use std::sync::Arc;

use context::adapters::DatasetCanonicalSessionWriter;
use context::domain::session::{
    AcceptedInputProjection, CanonicalSession, CommittedRunSlice, CommittedRunStep,
};
use share::message::Message;
use storage::api::{
    AtomicDatasetPort, DatasetKey, DatasetReadOutcome, SafePathSegment, StorageNamespace,
};
use storage::FileSystemDatasetAdapter;

fn session_with_steps(id: &str, revision: u64, steps: &[(&str, &str, &str)]) -> CanonicalSession {
    let mut session = CanonicalSession::fixture(id);
    session.revision = revision;
    session.run_slices = steps
        .iter()
        .map(|(run_id, step_id, text)| {
            CommittedRunSlice::new(
                *run_id,
                vec![CommittedRunStep::accepted_only(
                    *step_id,
                    AcceptedInputProjection::new(
                        vec![Message::user(*text)],
                        format!("{run_id}:{step_id}:{text}"),
                        revision,
                    ),
                )],
            )
        })
        .collect();
    session
}

fn dataset_key(session_id: &str) -> DatasetKey {
    DatasetKey::new(
        StorageNamespace::Session,
        vec![format!("{session_id}.dataset")
            .parse::<SafePathSegment>()
            .expect("safe session dataset id")],
    )
    .expect("valid Session dataset key")
}

#[tokio::test]
async fn save_incremental_maps_session_changes_and_reuses_unchanged_step_member() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let before = session_with_steps("session", 1, &[("run-a", "step-a", "a")]);

    writer
        .save_initial(&before)
        .await
        .expect("initial generation must commit");

    let unchanged_name = "step-72756e2d61-737465702d61.json";
    let unchanged_path = root
        .path()
        .join("session")
        .join("session.dataset")
        .join("primary")
        .join("blobs")
        .join(unchanged_name);
    #[cfg(unix)]
    let initial_inode = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&unchanged_path)
            .expect("initial unchanged member")
            .ino()
    };

    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.append_accepted_input(
        "run-b",
        "step-b",
        AcceptedInputProjection::new(vec![Message::user("b")], "run-b:step-b:b", 2),
    );
    writer
        .save_incremental(&before, &after)
        .await
        .expect("incremental generation must commit");

    let manifest = dataset
        .read_manifest(&dataset_key("session"))
        .await
        .expect("read committed manifest");
    let names = manifest
        .members()
        .iter()
        .map(SafePathSegment::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "manifest.json",
            "metadata.json",
            "session-state.json",
            unchanged_name,
            "step-72756e2d62-737465702d62.json",
        ]
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let current_inode = std::fs::metadata(&unchanged_path)
            .expect("reused unchanged member")
            .ino();
        assert_eq!(current_inode, initial_inode);
    }
}

#[tokio::test]
async fn save_incremental_with_stale_manifest_preserves_current_generation() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let before = session_with_steps("session", 1, &[("run", "step", "a")]);
    writer
        .save_initial(&before)
        .await
        .expect("initial generation must commit");

    let mut committed = before.clone();
    committed.revision = 2;
    committed.metadata.title = Some("committed".to_string());
    writer
        .save_incremental(&before, &committed)
        .await
        .expect("first incremental generation must commit");

    let mut stale_candidate = before.clone();
    stale_candidate.revision = 2;
    stale_candidate.metadata.title = Some("stale".to_string());
    let error = writer
        .save_incremental(&before, &stale_candidate)
        .await
        .expect_err("stale Session generation must be rejected");
    assert!(error.to_string().contains("修订号"));

    let manifest = dataset
        .read_manifest(&dataset_key("session"))
        .await
        .expect("read current generation");
    let requested = manifest.members().to_vec();
    let DatasetReadOutcome::Found(read) = dataset
        .read_consistent(&dataset_key("session"), &requested)
        .await
        .expect("current generation remains readable")
    else {
        panic!("current generation must remain committed");
    };
    let state = read
        .members()
        .iter()
        .find(|member| member.name().as_str() == "metadata.json")
        .expect("Session state member");
    let value: serde_json::Value = serde_json::from_slice(state.bytes()).expect("metadata JSON");
    assert_eq!(value["metadata"]["title"], "committed");
}
