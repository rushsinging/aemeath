use std::sync::Arc;

use context::adapters::{CanonicalSessionWriter, DatasetCanonicalSessionWriter};
use context::domain::session::{
    AcceptedInputRecord, ActiveCompactMarker, CanonicalSession, CommittedRunSlice,
    CommittedRunStep, FinalizedOutcomeRecord, RunStepCursor, SessionCommitPlan,
    SessionGenerationCodec,
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
                    AcceptedInputRecord::new(
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
    let initial_manifest = dataset
        .read_manifest(&dataset_key("session"))
        .await
        .expect("read initial manifest");
    let unchanged_member = initial_manifest
        .member_evidence(
            &unchanged_name
                .parse::<SafePathSegment>()
                .expect("safe unchanged member name"),
        )
        .expect("initial unchanged member evidence");
    let initial_content_files = std::fs::read_dir(
        root.path()
            .join("session")
            .join("session.dataset")
            .join("members"),
    )
    .expect("shared member store")
    .filter_map(Result::ok)
    .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
    .count();

    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.append_accepted_input(
        "run-b",
        "step-b",
        AcceptedInputRecord::new(vec![Message::user("b")], "run-b:step-b:b", 2),
    );
    writer
        .save_incremental(
            &before,
            &after,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
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

    let reused_member = manifest
        .member_evidence(
            &unchanged_name
                .parse::<SafePathSegment>()
                .expect("safe unchanged member name"),
        )
        .expect("reused unchanged member evidence");
    assert_eq!(
        reused_member.member_digest(),
        unchanged_member.member_digest()
    );
    let current_content_files = std::fs::read_dir(
        root.path()
            .join("session")
            .join("session.dataset")
            .join("members"),
    )
    .expect("shared member store")
    .filter_map(Result::ok)
    .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
    .count();
    assert!(current_content_files > initial_content_files);
}

#[tokio::test]
async fn overlay_step_missing_from_current_generation_is_written_not_reused() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let persisted = session_with_steps("overlay-session", 1, &[("run-a", "step-a", "a")]);
    writer
        .save_initial(&persisted)
        .await
        .expect("initial generation must commit");

    let mut before = persisted.clone();
    before.run_slices = before.run_slices.append_accepted_input(
        "run-overlay",
        "step-overlay",
        AcceptedInputRecord::new(vec![Message::user("overlay")], "overlay", 2),
    );
    let mut after = before.clone();
    after.revision = 2;
    after.metadata.title = Some("persist overlay".to_string());

    writer
        .save_incremental(
            &before,
            &after,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .expect("overlay member must be written without reuse evidence failure");

    let manifest = dataset
        .read_manifest(&dataset_key("overlay-session"))
        .await
        .expect("read committed generation");
    assert!(manifest.members().iter().any(|member| {
        member.as_str() == "step-72756e2d6f7665726c6179-737465702d6f7665726c6179.json"
    }));
}

#[tokio::test]
async fn accepted_input_mutation_writes_one_new_step_member_for_large_history() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let steps = (0..256)
        .map(|index| {
            (
                format!("run-{index}"),
                format!("step-{index}"),
                format!("history-{index}"),
            )
        })
        .collect::<Vec<_>>();
    let before_steps = steps
        .iter()
        .map(|(run_id, step_id, text)| (run_id.as_str(), step_id.as_str(), text.as_str()))
        .collect::<Vec<_>>();
    let before = session_with_steps("large-session", 1, &before_steps);

    writer
        .save_initial(&before)
        .await
        .expect("initial generation");
    let member_dir = root
        .path()
        .join("session")
        .join("large-session.dataset")
        .join("members");
    let initial_member_count = std::fs::read_dir(&member_dir)
        .expect("member store")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .count();

    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.append_accepted_input(
        "run-new",
        "step-new",
        AcceptedInputRecord::new(vec![Message::user("new input")], "new-input", 2),
    );
    writer
        .save_incremental(
            &before,
            &after,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .expect("accepted input mutation");

    let final_member_count = std::fs::read_dir(&member_dir)
        .expect("member store")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .count();
    assert_eq!(
        final_member_count - initial_member_count,
        3,
        "accepted input mutation may update metadata and state, but must not rewrite history members"
    );
    let manifest = dataset
        .read_manifest(&dataset_key("large-session"))
        .await
        .expect("committed manifest");
    let history_member_count = manifest
        .members()
        .iter()
        .filter(|member| member.as_str().starts_with("step-"))
        .count();
    assert_eq!(
        history_member_count, 257,
        "large-history accepted input must add exactly one step member"
    );
}

#[tokio::test]
async fn active_resume_append_reuses_compact_history_members() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let mut complete = session_with_steps(
        "resumed",
        7,
        &[
            ("run-old", "step-old", "old"),
            ("run-active", "step-active", "active"),
        ],
    );
    complete.compact = Some(ActiveCompactMarker {
        summary: "summary".to_string(),
        start_at: Some(RunStepCursor {
            run_id: "run-active".to_string(),
            step_id: "step-active".to_string(),
        }),
        source_revision: 6,
    });
    writer
        .save_initial(&complete)
        .await
        .expect("complete generation must commit");

    let mut active = complete.clone();
    active.run_slices = active
        .run_slices
        .iter()
        .filter(|slice| slice.run_id == "run-active")
        .map(|slice| slice.as_ref().clone())
        .collect();
    let mut appended = active.clone();
    appended.revision = 8;
    appended.run_slices = appended.run_slices.append_accepted_input(
        "run-next",
        "step-next",
        AcceptedInputRecord::new(vec![Message::user("next")], "next", 8),
    );

    writer
        .save_incremental(
            &active,
            &appended,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .expect("active Resume append must commit against complete generation");

    let manifest = dataset
        .read_manifest(&dataset_key("resumed"))
        .await
        .expect("read committed manifest");
    let names = manifest
        .members()
        .iter()
        .map(SafePathSegment::as_str)
        .collect::<Vec<_>>();
    assert!(names.contains(&"step-72756e2d6f6c64-737465702d6f6c64.json"));
    let manifest_name = "manifest.json"
        .parse::<SafePathSegment>()
        .expect("manifest member name");
    let DatasetReadOutcome::Found(read) = dataset
        .read_consistent(&dataset_key("resumed"), &[manifest_name])
        .await
        .expect("read domain manifest")
    else {
        panic!("domain manifest must remain committed");
    };
    let domain_manifest = SessionGenerationCodec::decode_manifest(read.members()[0].bytes())
        .expect("domain manifest");
    assert_eq!(domain_manifest.revision(), 8);
    assert_eq!(domain_manifest.steps().len(), 3);
    assert_eq!(domain_manifest.steps()[0].cursor().step_id, "step-old");
    assert_eq!(domain_manifest.steps()[2].cursor().step_id, "step-next");
}

#[tokio::test]
async fn active_resume_finalize_reuses_compact_history_members() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let mut complete = session_with_steps(
        "finalized-resume",
        7,
        &[
            ("run-old", "step-old", "old"),
            ("run-active", "step-active", "active"),
        ],
    );
    complete.compact = Some(ActiveCompactMarker {
        summary: "summary".to_string(),
        start_at: Some(RunStepCursor {
            run_id: "run-active".to_string(),
            step_id: "step-active".to_string(),
        }),
        source_revision: 6,
    });
    writer
        .save_initial(&complete)
        .await
        .expect("complete generation must commit");

    let mut active = complete.clone();
    active.run_slices = active
        .run_slices
        .iter()
        .filter(|slice| slice.run_id == "run-active")
        .map(|slice| slice.as_ref().clone())
        .collect();
    let mut finalized = active.clone();
    finalized.revision = 8;
    finalized.append_finalized_outcome(
        "run-active",
        "step-active",
        FinalizedOutcomeRecord::compatibility(vec![Message::user("done")]),
    );

    writer
        .save_incremental(
            &active,
            &finalized,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .expect("active Resume finalize must preserve complete generation");

    let manifest_name = "manifest.json"
        .parse::<SafePathSegment>()
        .expect("manifest member name");
    let DatasetReadOutcome::Found(read) = dataset
        .read_consistent(&dataset_key("finalized-resume"), &[manifest_name])
        .await
        .expect("read domain manifest")
    else {
        panic!("domain manifest must remain committed");
    };
    let manifest = SessionGenerationCodec::decode_manifest(read.members()[0].bytes())
        .expect("domain manifest");
    assert_eq!(manifest.revision(), 8);
    assert_eq!(manifest.steps().len(), 2);
    assert_eq!(manifest.steps()[0].cursor().step_id, "step-old");
    assert_eq!(manifest.steps()[1].cursor().step_id, "step-active");
}

#[tokio::test]
async fn partial_history_commit_intent_cannot_remove_persisted_step_members() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let before = session_with_steps(
        "partial-history",
        1,
        &[
            ("run-old", "step-old", "old"),
            ("run-live", "step-live", "live"),
        ],
    );
    writer
        .save_initial(&before)
        .await
        .expect("initial generation must commit");
    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.cleared();

    writer
        .save_incremental(
            &before,
            &after,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .expect("partial history commit must preserve persisted steps");

    let manifest = dataset
        .read_manifest(&dataset_key("partial-history"))
        .await
        .expect("read committed manifest");
    let step_count = manifest
        .members()
        .iter()
        .filter(|member| member.as_str().starts_with("step-"))
        .count();
    assert_eq!(step_count, 2);
}

#[tokio::test]
async fn complete_history_replacement_removes_every_step_member() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let before = session_with_steps(
        "cleared",
        1,
        &[("run-a", "step-a", "a"), ("run-b", "step-b", "b")],
    );
    writer
        .save_initial(&before)
        .await
        .expect("initial generation must commit");
    let mut after = before.clone();
    after.revision = 2;
    after.run_slices = after.run_slices.cleared();

    writer
        .save_incremental(
            &before,
            &after,
            context::adapters::SessionSaveIntent::ReplaceCompleteHistory,
        )
        .await
        .expect("complete history replacement must commit");

    let manifest = dataset
        .read_manifest(&dataset_key("cleared"))
        .await
        .expect("read committed manifest");
    assert_eq!(
        manifest
            .members()
            .iter()
            .map(SafePathSegment::as_str)
            .collect::<Vec<_>>(),
        ["manifest.json", "metadata.json", "session-state.json"]
    );
}

#[tokio::test]
async fn commit_plan_publishes_first_generation_when_dataset_is_absent() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let before = session_with_steps("new-session", 0, &[]);
    let mut after = before.clone();
    after.revision = 1;
    after.run_slices = after.run_slices.append_accepted_input(
        "run",
        "step",
        AcceptedInputRecord::new(vec![Message::user("first")], "first", 1),
    );
    let plan = SessionCommitPlan::between(&before, &after).expect("first commit plan");

    writer
        .commit_plan("new-session", 0, plan)
        .await
        .expect("first typed commit must publish an initial Dataset generation");

    let manifest_name = "manifest.json"
        .parse::<SafePathSegment>()
        .expect("manifest member name");
    let DatasetReadOutcome::Found(read) = dataset
        .read_consistent(&dataset_key("new-session"), &[manifest_name])
        .await
        .expect("read first generation manifest")
    else {
        panic!("first Dataset generation must exist");
    };
    let manifest = SessionGenerationCodec::decode_manifest(read.members()[0].bytes())
        .expect("domain manifest");
    assert_eq!(manifest.session_id(), "new-session");
    assert_eq!(manifest.revision(), 1);
    assert_eq!(manifest.steps().len(), 1);
}

#[tokio::test]
async fn commit_plan_rejects_session_identity_mismatch_before_dataset_publish() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let before = session_with_steps("session", 1, &[("run", "step", "before")]);
    writer
        .save_initial(&before)
        .await
        .expect("initial generation must commit");

    let mut after = before.clone();
    after.id = "foreign-session".to_string();
    after.revision = 2;
    let plan = SessionCommitPlan::between(&after, &after).expect("foreign plan");

    let error = writer
        .commit_plan("session", 1, plan)
        .await
        .expect_err("plan identity must match the committed dataset");

    assert!(error.contains("identity"), "unexpected error: {error}");
    let manifest = dataset
        .read_manifest(&dataset_key("session"))
        .await
        .expect("read unchanged manifest");
    let requested = manifest.members().to_vec();
    let DatasetReadOutcome::Found(read) = dataset
        .read_consistent(&dataset_key("session"), &requested)
        .await
        .expect("current generation remains readable")
    else {
        panic!("current generation must remain committed");
    };
    let metadata = read
        .members()
        .iter()
        .find(|member| member.name().as_str() == "metadata.json")
        .expect("metadata member");
    let value: serde_json::Value = serde_json::from_slice(metadata.bytes()).expect("metadata JSON");
    assert_eq!(value["id"], "session");
    assert_eq!(value["revision"], 1);
}

#[tokio::test]
async fn commit_plan_rejects_target_revision_that_does_not_advance_expected_revision() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());
    let before = session_with_steps("session", 1, &[("run", "step", "before")]);
    writer
        .save_initial(&before)
        .await
        .expect("initial generation must commit");
    let mut invalid_after = before.clone();
    invalid_after.metadata.title = Some("must not publish".to_string());
    let plan = SessionCommitPlan::between(&before, &invalid_after).expect("non-advancing plan");

    let error = writer
        .commit_plan("session", 1, plan)
        .await
        .expect_err("target revision must advance the expected revision");

    assert!(error.contains("revision"), "unexpected error: {error}");
    let manifest_name = "manifest.json"
        .parse::<SafePathSegment>()
        .expect("manifest member name");
    let DatasetReadOutcome::Found(read) = dataset
        .read_consistent(&dataset_key("session"), &[manifest_name])
        .await
        .expect("read unchanged generation manifest")
    else {
        panic!("current generation must remain committed");
    };
    let manifest = SessionGenerationCodec::decode_manifest(read.members()[0].bytes())
        .expect("domain manifest");
    assert_eq!(manifest.revision(), 1);
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
        .save_incremental(
            &before,
            &committed,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
        .await
        .expect("first incremental generation must commit");

    let mut stale_candidate = before.clone();
    stale_candidate.revision = 2;
    stale_candidate.metadata.title = Some("stale".to_string());
    let error = writer
        .save_incremental(
            &before,
            &stale_candidate,
            context::adapters::SessionSaveIntent::CommitPartialHistory,
        )
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

#[tokio::test]
async fn rebuild_empty_dataset_restores_wiped_dataset_with_aligned_revision() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());

    // 模拟外部清空：先落一代，再删除数据集目录。
    let seeded = session_with_steps("session", 1, &[("run-a", "step-a", "a")]);
    writer
        .save_initial(&seeded)
        .await
        .expect("seed generation before wipe");
    std::fs::remove_dir_all(root.path().join("session").join("session.dataset"))
        .expect("wipe dataset directory");

    let wiped_session = session_with_steps(
        "session",
        26,
        &[("run-a", "step-a", "a"), ("run-b", "step-b", "b")],
    );
    writer
        .rebuild_empty_dataset("session", &wiped_session)
        .await
        .expect("wiped dataset must be rebuilt from the in-memory session");

    let manifest = dataset
        .read_manifest(&dataset_key("session"))
        .await
        .expect("read rebuilt manifest");
    assert!(
        !manifest.members().is_empty(),
        "rebuilt dataset must carry members"
    );
    let state_name = "session-state.json"
        .parse::<SafePathSegment>()
        .expect("safe state member name");
    let read = dataset
        .read_consistent(&dataset_key("session"), &[state_name])
        .await
        .expect("read rebuilt members");
    let DatasetReadOutcome::Found(read) = read else {
        panic!("rebuilt generation must be readable");
    };
    assert_eq!(read.members().len(), 1);

    // 重建后磁盘 manifest revision 必须与内存对齐：后续增量提交不再冲突。
    let mut after = wiped_session.clone();
    after.revision = 27;
    writer
        .save_incremental(
            &wiped_session,
            &after,
            context::adapters::SessionSaveIntent::ReplaceCompleteHistory,
        )
        .await
        .expect("incremental commit after rebuild must align with rebuilt revision");
}

#[tokio::test]
async fn rebuild_empty_dataset_fails_closed_when_dataset_is_not_empty() {
    let root = tempfile::tempdir().expect("temporary dataset root");
    let dataset = Arc::new(FileSystemDatasetAdapter::new(root.path()).expect("dataset adapter"));
    let writer = DatasetCanonicalSessionWriter::new(dataset.clone());

    let seeded = session_with_steps("session", 1, &[("run-a", "step-a", "a")]);
    writer.save_initial(&seeded).await.expect("seed generation");

    let outcome = writer
        .rebuild_empty_dataset("session", &seeded)
        .await
        .expect_err("non-empty dataset must fail closed instead of being overwritten");
    assert!(
        outcome.contains("非空"),
        "failure must explain the dataset is not empty: {outcome}"
    );
}
