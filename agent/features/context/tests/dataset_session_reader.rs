use std::sync::Arc;

use context::adapters::{DatasetCanonicalSessionWriter, DatasetSessionReader};
use context::domain::session::{
    AcceptedInputProjection, CanonicalSession, CommittedRunSlice, CommittedRunStep,
    SessionGenerationCodec, SessionGenerationManifest,
};
use share::message::Message;
use storage::api::{
    AtomicDatasetPort, DatasetKey, DatasetMember, Durability, SafePathSegment, StorageNamespace,
    WriteOptions,
};
use storage::FileSystemDatasetAdapter;

fn session_with_step(id: &str, revision: u64, text: &str) -> CanonicalSession {
    let mut session = CanonicalSession::fixture(id);
    session.revision = revision;
    session.run_slices = vec![CommittedRunSlice::new(
        "run",
        vec![CommittedRunStep::accepted_only(
            "step",
            AcceptedInputProjection::new(vec![Message::user(text)], text, revision),
        )],
    )]
    .into();
    session
}

fn dataset_key(session_id: &str) -> DatasetKey {
    DatasetKey::new(
        StorageNamespace::Session,
        vec![format!("{session_id}.dataset")
            .parse::<SafePathSegment>()
            .expect("safe session dataset id")],
    )
    .expect("dataset key")
}

#[tokio::test]
async fn dataset_reader_migrates_legacy_blob_once_when_dataset_is_absent() {
    let root = tempfile::tempdir().expect("temporary root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let blob: Arc<dyn storage::api::AtomicBlobPort> =
        Arc::new(storage::FileSystemBlobAdapter::new(root.path()).expect("blob adapter"));
    let expected = session_with_step("legacy", 4, "legacy history");
    let legacy_management = context::adapters::AtomicBlobSessionManagement::new(blob.clone());
    let project = share::session_types::ProjectIdentity {
        initial_cwd: "/legacy".to_string(),
        git_common_dir: None,
    };
    let mut expected = expected;
    expected.workspace = context::domain::session::SnapshotState::Captured(
        share::session_types::PersistedWorkspaceContext {
            workspace_id: share::session_types::WorkspaceId::derive(&project, "/legacy"),
            project_identity: project.clone(),
            path_base: "/legacy".to_string(),
            workspace_root: "/legacy".to_string(),
            worktree_kind: share::session_types::WorktreeKind::Primary,
            context_stack: Vec::new(),
        },
    );
    context::SessionManagementPort::import_for_project(
        &legacy_management,
        &context::domain::session::SessionCodec::encode(&expected).expect("encode legacy"),
        &project,
    )
    .await
    .expect("save legacy blob");

    let reader = DatasetSessionReader::new(dataset.clone(), Some(blob));
    let loaded = reader
        .load("legacy")
        .await
        .expect("load and migrate legacy");

    assert_eq!(loaded, expected);
    assert!(!dataset
        .read_manifest(&dataset_key("legacy"))
        .await
        .expect("migrated dataset manifest")
        .members()
        .is_empty());
}

#[tokio::test]
async fn dataset_reader_restores_primary_generation_without_legacy_blob() {
    let root = tempfile::tempdir().expect("temporary root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let expected = session_with_step("primary", 3, "primary history");
    writer
        .save_initial(&expected)
        .await
        .expect("save generation");

    let reader = DatasetSessionReader::new(dataset, None);
    let loaded = reader
        .load("primary")
        .await
        .expect("load primary generation");

    assert_eq!(loaded, expected);
}

#[tokio::test]
async fn dataset_reader_falls_back_to_previous_when_primary_domain_manifest_is_invalid() {
    let root = tempfile::tempdir().expect("temporary root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let previous = session_with_step("recover", 1, "previous history");
    writer
        .save_initial(&previous)
        .await
        .expect("save previous generation");

    let current = session_with_step("recover", 2, "current history");
    writer
        .save_incremental(
            &previous,
            &current,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .expect("save current generation");

    let manifest_name = SessionGenerationManifest::manifest_member_name()
        .parse::<SafePathSegment>()
        .expect("manifest name");
    let corrupt_members = vec![DatasetMember::new(manifest_name, b"not-json".to_vec())];
    let storage_manifest = dataset
        .read_manifest(&dataset_key("recover"))
        .await
        .expect("current storage manifest");
    dataset
        .commit_atomic(
            &dataset_key("recover"),
            storage_manifest.revision(),
            &corrupt_members,
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await
        .expect("publish invalid domain generation");

    let reader = DatasetSessionReader::new(dataset, None);
    let loaded = reader
        .load("recover")
        .await
        .expect("recover previous generation");

    assert_eq!(loaded, current);
    assert_eq!(
        loaded.structured_messages()[0].text_content(),
        "current history"
    );
}

#[tokio::test]
async fn dataset_reader_reports_future_manifest_and_preserves_original_bytes() {
    let root = tempfile::tempdir().expect("temporary root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let future_bytes = br#"{"generation_schema_version":999,"opaque":"keep-me"}"#.to_vec();
    let manifest_name = SessionGenerationManifest::manifest_member_name()
        .parse::<SafePathSegment>()
        .expect("manifest name");
    let empty = dataset
        .read_manifest(&dataset_key("future"))
        .await
        .expect("empty manifest");
    dataset
        .commit_atomic(
            &dataset_key("future"),
            empty.revision(),
            &[DatasetMember::new(manifest_name, future_bytes.clone())],
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await
        .expect("publish future manifest");

    let reader = DatasetSessionReader::new(dataset, None);
    let error = reader
        .load("future")
        .await
        .expect_err("future schema fails closed");

    assert!(matches!(
        error,
        context::domain::session::SessionGenerationWireError::UnsupportedFutureVersion {
            version: 999,
            original_bytes,
        } if original_bytes == future_bytes
    ));
}

#[tokio::test]
async fn dataset_reader_loads_only_steps_after_compact_marker_for_runtime_resume() {
    let root = tempfile::tempdir().expect("temporary root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let mut session = session_with_step("compacted", 7, "hidden before compact");
    session.run_slices = vec![
        CommittedRunSlice::new(
            "run-1",
            vec![CommittedRunStep::accepted_only(
                "step-1",
                AcceptedInputProjection::new(
                    vec![Message::user("hidden before compact")],
                    "fp-1",
                    1,
                ),
            )],
        ),
        CommittedRunSlice::new(
            "run-2",
            vec![CommittedRunStep::accepted_only(
                "step-2",
                AcceptedInputProjection::new(
                    vec![Message::user("visible after compact")],
                    "fp-2",
                    7,
                ),
            )],
        ),
    ]
    .into();
    session.compact = Some(context::domain::session::ActiveCompactMarker {
        summary: "compacted summary".to_string(),
        start_at: Some(context::domain::session::RunStepCursor {
            run_id: "run-2".to_string(),
            step_id: "step-2".to_string(),
        }),
        source_revision: 6,
    });
    writer
        .save_initial(&session)
        .await
        .expect("save generation");

    let reader = DatasetSessionReader::new(dataset, None);
    let prepared = reader
        .load_for_resume("compacted")
        .await
        .expect("load compacted generation");
    let loaded = prepared.active_session;

    assert_eq!(loaded.run_slices.len(), 1);
    assert_eq!(loaded.run_slices[0].steps.len(), 1);
    assert_eq!(loaded.run_slices[0].steps[0].step_id, "step-2");
    assert_eq!(
        loaded.structured_messages()[0].text_content(),
        "visible after compact"
    );
    assert_eq!(prepared.display_history.steps().len(), 2);
    assert_eq!(prepared.display_history.steps()[0].run_id(), "run-1");
    assert_eq!(prepared.display_history.steps()[0].step_id(), "step-1");
    assert_eq!(prepared.display_history.steps()[1].run_id(), "run-2");
    assert_eq!(prepared.display_history.steps()[1].step_id(), "step-2");
}

#[tokio::test]
async fn dataset_reader_loads_requested_display_history_steps_from_same_generation() {
    let root = tempfile::tempdir().expect("temporary root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let mut session = session_with_step("windowed", 9, "first");
    session.run_slices = vec![
        CommittedRunSlice::new(
            "run-1",
            vec![CommittedRunStep::accepted_only(
                "step-1",
                AcceptedInputProjection::new(vec![Message::user("first body")], "fp-1", 1),
            )],
        ),
        CommittedRunSlice::new(
            "run-2",
            vec![CommittedRunStep::accepted_only(
                "step-2",
                AcceptedInputProjection::new(vec![Message::user("second body")], "fp-2", 9),
            )],
        ),
    ]
    .into();
    writer
        .save_initial(&session)
        .await
        .expect("save generation");

    let reader = DatasetSessionReader::new(dataset, None);
    let prepared = reader
        .load_for_resume("windowed")
        .await
        .expect("prepare resume");
    let window = reader
        .load_display_history_steps(
            "windowed",
            prepared.display_history.generation_revision(),
            &[prepared.display_history.steps()[0]
                .member_name()
                .to_string()],
        )
        .await
        .expect("load requested history window");

    assert_eq!(window.session_id(), "windowed");
    assert_eq!(window.generation_revision(), 9);
    assert_eq!(window.steps().len(), 1);
    assert_eq!(window.steps()[0].cursor().run_id, "run-1");
    assert_eq!(window.steps()[0].cursor().step_id, "step-1");
    assert_eq!(
        window.steps()[0]
            .step()
            .accepted_input
            .as_ref()
            .expect("accepted input")
            .messages[0]
            .text_content(),
        "first body"
    );
}

#[test]
fn manifest_codec_fixture_remains_current_for_reader_contract() {
    let manifest = SessionGenerationManifest::new("fixture", 0, vec![]).expect("manifest");
    let encoded = SessionGenerationCodec::encode_manifest(&manifest).expect("encode manifest");
    assert_eq!(
        SessionGenerationCodec::decode_manifest(&encoded)
            .expect("decode manifest")
            .session_id(),
        "fixture"
    );
}
