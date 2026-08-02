use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use context::adapters::{CanonicalSessionRepository, CanonicalSessionWriter};
use context::domain::session::{
    AcceptedInputProjection, CanonicalSession, ChatSegment, CommittedRunSlice, CommittedRunStep,
    SnapshotState,
};
use context::domain::{
    AcceptedInputAppend, AcceptedInputError, CompactRequest, CompactTrigger, ContentFingerprint,
    ContextAppend, ContextAppendError, ContextRequest, ContextRequestId, FinalizeCause, Language,
    ManualCompactRequest, RunStepId, SessionId, SessionRevision, SystemPromptSpec,
    ToolCallIdentity, ToolCallState, ToolReceiptMutation,
};
use context::ports::SessionRepository;
use project::{PreparedWorkspaceRestore, WorkspacePersist, WorkspaceRestoreError};
use provider::ReasoningLevel;
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;
use share::session_types::{PersistedWorkspaceContext, ProjectIdentity, WorkspaceId, WorktreeKind};
use task::{PreparedTaskRestore, TaskPersist, TaskSnapshot, TaskSnapshotValidationError};

use tools::{SkillLoadDecision, SkillLoadMutation, SkillLoadScope, SkillLoadStateError};

#[derive(Default)]
struct RecordingWriter {
    saved: Mutex<Vec<CanonicalSession>>,
    fail: bool,
}

#[async_trait]
impl CanonicalSessionWriter for RecordingWriter {
    async fn save(
        &self,
        _before: &CanonicalSession,
        session: &CanonicalSession,
    ) -> Result<(), String> {
        if self.fail {
            return Err("disk full".to_string());
        }
        self.saved.lock().unwrap().push(session.clone());
        Ok(())
    }
}

struct EmptyTask;
impl TaskPersist for EmptyTask {
    fn collect_snapshot(&self) -> TaskSnapshot {
        TaskSnapshot::empty()
    }

    fn prepare_restore(
        &self,
        snapshot: &TaskSnapshot,
    ) -> Result<PreparedTaskRestore, TaskSnapshotValidationError> {
        task::wire_task().persist().prepare_restore(snapshot)
    }

    fn commit_restore(&self, _token: PreparedTaskRestore) {}
}

struct FixedWorkspace(PersistedWorkspaceContext);
impl WorkspacePersist for FixedWorkspace {
    fn snapshot(&self) -> PersistedWorkspaceContext {
        self.0.clone()
    }

    fn prepare_restore(
        &self,
        _dto: &PersistedWorkspaceContext,
    ) -> Result<PreparedWorkspaceRestore, WorkspaceRestoreError> {
        panic!("not used")
    }

    fn commit_restore(&self, _prepared: PreparedWorkspaceRestore) {
        panic!("not used")
    }
}

fn workspace() -> PersistedWorkspaceContext {
    let project_identity = ProjectIdentity {
        initial_cwd: "/tmp/project".to_string(),
        git_common_dir: None,
    };
    PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&project_identity, "/tmp/project"),
        project_identity,
        path_base: "/tmp/project".to_string(),
        workspace_root: "/tmp/project".to_string(),
        worktree_kind: WorktreeKind::Primary,
        context_stack: vec![],
    }
}

fn append(fingerprint: &str) -> ContextAppend {
    ContextAppend {
        session_id: SessionId::new("session"),
        expected_revision: SessionRevision::new(0),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: ContextRequestId::new("request"),
        finalize_cause: FinalizeCause::Completed,
        duration_ms: None,
        messages: vec![Message::user("fact")],
        receipts: vec![],
        api_input_tokens: None,
        fingerprint: ContentFingerprint::new(fingerprint),
    }
}

fn tool_identity() -> ToolCallIdentity {
    ToolCallIdentity {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        runtime_call_id: "call-1".to_string(),
        provider_call_id: Some("provider-1".to_string()),
        tool_name: "Glob".to_string(),
        call_index: 0,
        agent: false,
    }
}

fn accepted_input(fingerprint: &str) -> AcceptedInputAppend {
    AcceptedInputAppend {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: ContextRequestId::new("request"),
        messages: vec![Message::user("accepted fact")],
        fingerprint: ContentFingerprint::new(fingerprint),
    }
}

fn compact_request(session_id: SessionId) -> ContextRequest {
    ContextRequest {
        session_id,
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".to_string(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::new("zh"),
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 1,
        max_output_tokens: 1,
        last_api_total_tokens: Some(100),
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

fn repository_with_session(
    writer: Arc<RecordingWriter>,
    session: CanonicalSession,
) -> (
    CanonicalSessionRepository,
    Arc<RwLock<Arc<CanonicalSession>>>,
) {
    let holder = Arc::new(RwLock::new(Arc::new(session)));
    (
        CanonicalSessionRepository::new(
            holder.clone(),
            Arc::new(EmptyTask),
            Arc::new(FixedWorkspace(workspace())),
            writer,
            Arc::new(tokio::sync::Mutex::new(())),
        ),
        holder,
    )
}

fn repository(
    writer: Arc<RecordingWriter>,
) -> (
    CanonicalSessionRepository,
    Arc<RwLock<Arc<CanonicalSession>>>,
) {
    let session_id = SessionId::new("session");
    repository_with_session(
        writer,
        CanonicalSession {
            id: session_id.to_string(),
            chats: vec![],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            metadata: Default::default(),
            tasks: SnapshotState::Missing,
            workspace: SnapshotState::Captured(workspace()),
            revision: 0,
            compact: None,
            run_slices: vec![].into(),
            committed_steps: Default::default(),
            skill_load_records: Vec::new(),
        },
    )
}

fn ten_step_slices() -> Vec<CommittedRunSlice> {
    (0..10)
        .map(|index| {
            CommittedRunSlice::new(
                format!("run-{index}"),
                vec![CommittedRunStep::compatibility_outcome_only(
                    format!("step-{index}"),
                    vec![Message::user(format!("message-{index}"))],
                )],
            )
        })
        .collect()
}

fn ten_step_session(
    session_id: &SessionId,
    chats: Vec<ChatSegment>,
    revision: u64,
) -> CanonicalSession {
    CanonicalSession {
        id: session_id.to_string(),
        chats,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(workspace()),
        revision,
        compact: None,
        run_slices: ten_step_slices().into(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    }
}

async fn compact(repository: &CanonicalSessionRepository, session_id: SessionId, revision: u64) {
    let request = compact_request(session_id);
    let outcome = repository
        .commit_compaction(&CompactRequest {
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(revision),
            source: request,
            trigger: CompactTrigger::Automatic,
        })
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        context::domain::CompactOutcome::Committed(_)
    ));
}

#[tokio::test]
async fn skill_load_revision_is_atomic_idempotent_and_failure_safe() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository_under_test, holder) = repository(writer.clone());
    let session_id = holder.read().unwrap().id.clone();
    let mutation = SkillLoadMutation::new(
        session_id,
        SkillLoadScope::main(),
        "superpowers:brainstorming",
        "r1",
    )
    .unwrap();

    assert_eq!(
        repository_under_test
            .compare_and_record_skill_load(mutation.clone())
            .await
            .unwrap(),
        SkillLoadDecision::Fresh
    );
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(writer.saved.lock().unwrap().len(), 1);

    assert_eq!(
        repository_under_test
            .compare_and_record_skill_load(mutation)
            .await
            .unwrap(),
        SkillLoadDecision::AlreadyLoaded
    );
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(writer.saved.lock().unwrap().len(), 1);

    let updated = SkillLoadMutation::new(
        holder.read().unwrap().id.clone(),
        SkillLoadScope::main(),
        "superpowers:brainstorming",
        "r2",
    )
    .unwrap();
    assert_eq!(
        repository_under_test
            .compare_and_record_skill_load(updated)
            .await
            .unwrap(),
        SkillLoadDecision::Updated
    );
    assert_eq!(holder.read().unwrap().revision, 2);
    assert_eq!(
        holder
            .read()
            .unwrap()
            .loaded_skill_revision(&SkillLoadScope::main(), "superpowers:brainstorming"),
        Some("r2")
    );

    let failing_writer = Arc::new(RecordingWriter {
        saved: Mutex::new(Vec::new()),
        fail: true,
    });
    let (failing, failing_holder) = repository(failing_writer);
    let failing_session_id = failing_holder.read().unwrap().id.clone();
    let failed = failing
        .compare_and_record_skill_load(
            SkillLoadMutation::new(failing_session_id, SkillLoadScope::main(), "review", "r1")
                .unwrap(),
        )
        .await;
    assert!(matches!(
        failed,
        Err(SkillLoadStateError::Storage(message)) if message == "disk full"
    ));
    assert_eq!(failing_holder.read().unwrap().revision, 0);
    assert!(failing_holder.read().unwrap().skill_load_records.is_empty());
}

#[tokio::test]
async fn skill_load_rejects_foreign_session_identity() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let error = repository
        .compare_and_record_skill_load(
            SkillLoadMutation::new("foreign", SkillLoadScope::main(), "review", "r1").unwrap(),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        SkillLoadStateError::SessionNotFound("foreign".to_string())
    );
    assert!(holder.read().unwrap().skill_load_records.is_empty());
}

#[tokio::test]
async fn clear_removes_persisted_skill_load_records() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let session_id = holder.read().unwrap().id.clone();
    repository
        .compare_and_record_skill_load(
            SkillLoadMutation::new(session_id.clone(), SkillLoadScope::main(), "review", "r1")
                .unwrap(),
        )
        .await
        .unwrap();

    repository
        .clear(&SessionId::new(&session_id))
        .await
        .unwrap();

    assert!(holder.read().unwrap().skill_load_records.is_empty());
}

#[tokio::test]
async fn compaction_preserves_skill_load_records() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let session_id = holder.read().unwrap().id.clone();
    repository
        .compare_and_record_skill_load(
            SkillLoadMutation::new(session_id.clone(), SkillLoadScope::main(), "review", "r1")
                .unwrap(),
        )
        .await
        .unwrap();
    repository
        .commit_compaction(&CompactRequest {
            run_id: RunId::new("run"),
            source_revision: SessionRevision::new(1),
            source: compact_request(SessionId::new(&session_id)),
            trigger: CompactTrigger::Automatic,
        })
        .await
        .unwrap();

    assert_eq!(
        holder
            .read()
            .unwrap()
            .loaded_skill_revision(&SkillLoadScope::main(), "review"),
        Some("r1")
    );
}

#[tokio::test]
async fn concurrent_skill_load_records_return_one_fresh_decision() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let repository = Arc::new(repository);
    let mutation = SkillLoadMutation::new(
        holder.read().unwrap().id.clone(),
        SkillLoadScope::main(),
        "review",
        "r1",
    )
    .unwrap();
    let first = {
        let repository = repository.clone();
        let mutation = mutation.clone();
        tokio::spawn(async move {
            repository
                .compare_and_record_skill_load(mutation)
                .await
                .unwrap()
        })
    };
    let second = {
        let repository = repository.clone();
        tokio::spawn(async move {
            repository
                .compare_and_record_skill_load(mutation)
                .await
                .unwrap()
        })
    };
    let decisions = [first.await.unwrap(), second.await.unwrap()];

    assert_eq!(
        decisions
            .iter()
            .filter(|decision| **decision == SkillLoadDecision::Fresh)
            .count(),
        1
    );
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| **decision == SkillLoadDecision::AlreadyLoaded)
            .count(),
        1
    );
}

#[cfg(feature = "dev")]
fn session_with_tool_result(session_id: &SessionId, revision: u64) -> CanonicalSession {
    let tool_result =
        Message::tool_results(vec![("tool-1".to_string(), "payload".to_string(), false)]);
    CanonicalSession {
        id: session_id.to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(workspace()),
        revision,
        compact: None,
        run_slices: vec![CommittedRunSlice::new(
            "run",
            vec![CommittedRunStep {
                step_id: "step".to_string(),
                accepted_input: Some(AcceptedInputProjection::new(
                    vec![Message::user("accepted")],
                    "input",
                    revision,
                )),
                outcome: Some(
                    context::domain::session::FinalizedOutcomeProjection::compatibility(vec![
                        tool_result,
                    ]),
                ),
                tool_receipts: Vec::new(),
            }],
        )]
        .into(),
        committed_steps: vec![context::domain::session::CommittedStep {
            run_id: "run".to_string(),
            step_id: "step".to_string(),
            fingerprint: "outcome".to_string(),
            committed_revision: revision,
        }]
        .into(),
        skill_load_records: Vec::new(),
    }
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn lifecycle_capture_reports_structure_and_releases_replaced_generation() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("lifecycle-session");
    let (repository, holder) =
        repository_with_session(writer, session_with_tool_result(&session_id, 1));

    let mut appended = append("next");
    appended.session_id = session_id;
    appended.expected_revision = SessionRevision::new(1);
    appended.run_id = RunId::new("run-2");
    appended.step_id = RunStepId::new("step-2");

    let (result, lifecycle) =
        context::adapters::capture_session_lifecycle(repository.append_finalized(&appended)).await;
    result.unwrap();

    assert_eq!(lifecycle.transitions.len(), 1);
    let transition = &lifecycle.transitions[0];
    assert_eq!(transition.before.revision, 1);
    assert_eq!(transition.before.run_slices, 1);
    assert_eq!(transition.before.steps, 1);
    assert_eq!(transition.before.accepted_messages, 1);
    assert_eq!(transition.before.outcome_messages, 1);
    assert_eq!(transition.before.tool_result_blocks, 1);
    assert_eq!(transition.before.tool_result_content_bytes, 9);
    assert_eq!(transition.before.committed_steps, 1);
    assert_eq!(transition.after.revision, 2);
    assert_eq!(transition.after.run_slices, 2);
    assert_eq!(transition.after.steps, 2);
    assert_eq!(transition.after.outcome_messages, 2);
    assert_eq!(transition.after.committed_steps, 2);
    assert!(transition.replaced_generation.upgrade().is_none());
    assert_eq!(holder.read().unwrap().revision, 2);
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn lifecycle_weak_probe_stays_live_until_external_arc_is_dropped() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let external = holder.read().unwrap().clone();

    let (result, lifecycle) =
        context::adapters::capture_session_lifecycle(repository.append_finalized(&append("same")))
            .await;
    result.unwrap();

    let replaced = lifecycle.transitions[0].replaced_generation.clone();
    assert!(replaced.upgrade().is_some());
    drop(external);
    assert!(replaced.upgrade().is_none());
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn snapshot_does_not_publish_a_new_session_generation() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let committed = holder.read().unwrap().clone();

    let (snapshot, lifecycle) = context::adapters::capture_session_lifecycle(
        repository.snapshot(&SessionId::new("session")),
    )
    .await;
    snapshot.unwrap();

    assert!(lifecycle.transitions.is_empty());
    assert!(Arc::ptr_eq(&committed, &holder.read().unwrap()));
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn clear_reports_zero_persisted_structure_and_releases_old_generation() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("clear-lifecycle-session");
    let (repository, _) = repository_with_session(writer, session_with_tool_result(&session_id, 3));

    let (result, lifecycle) =
        context::adapters::capture_session_lifecycle(repository.clear(&session_id)).await;
    result.unwrap();

    let transition = &lifecycle.transitions[0];
    assert_eq!(transition.after.revision, 4);
    assert_eq!(transition.after.run_slices, 0);
    assert_eq!(transition.after.steps, 0);
    assert_eq!(transition.after.accepted_messages, 0);
    assert_eq!(transition.after.outcome_messages, 0);
    assert_eq!(transition.after.tool_result_blocks, 0);
    assert_eq!(transition.after.tool_result_content_bytes, 0);
    assert_eq!(transition.after.committed_steps, 0);
    assert!(transition.replaced_generation.upgrade().is_none());
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn compaction_changes_visibility_without_dropping_persisted_structure() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("compact-lifecycle-session");
    let (repository, _) = repository_with_session(writer, ten_step_session(&session_id, vec![], 0));

    let request = compact_request(session_id);
    let (result, lifecycle) = context::adapters::capture_session_lifecycle(
        repository.commit_compaction(&CompactRequest {
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: CompactTrigger::Automatic,
        }),
    )
    .await;
    result.unwrap();

    let transition = &lifecycle.transitions[0];
    assert_eq!(transition.before.run_slices, 10);
    assert_eq!(transition.after.run_slices, 10);
    assert_eq!(transition.before.steps, 10);
    assert_eq!(transition.after.steps, 10);
    assert_eq!(transition.before.outcome_messages, 10);
    assert_eq!(transition.after.outcome_messages, 10);
    assert!(transition.replaced_generation.upgrade().is_none());
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn lifecycle_workload_counts_100_500_and_1000_committed_steps() {
    for step_count in [100usize, 500, 1_000] {
        let writer = Arc::new(RecordingWriter::default());
        let session_id = SessionId::new(format!("lifecycle-{step_count}"));
        let run_slices = (0..step_count)
            .map(|index| {
                CommittedRunSlice::new(
                    format!("run-{index}"),
                    vec![CommittedRunStep::compatibility_outcome_only(
                        format!("step-{index}"),
                        vec![Message::user(format!("message-{index}"))],
                    )],
                )
            })
            .collect();
        let session = CanonicalSession {
            id: session_id.to_string(),
            chats: vec![],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            metadata: Default::default(),
            tasks: SnapshotState::Missing,
            workspace: SnapshotState::Captured(workspace()),
            revision: step_count as u64,
            compact: None,
            run_slices,
            committed_steps: Default::default(),
            skill_load_records: Vec::new(),
        };
        let (repository, _) = repository_with_session(writer, session);
        let mut appended = append("tail");
        appended.session_id = session_id;
        appended.expected_revision = SessionRevision::new(step_count as u64);
        appended.run_id = RunId::new("tail-run");
        appended.step_id = RunStepId::new("tail-step");

        let (result, lifecycle) =
            context::adapters::capture_session_lifecycle(repository.append_finalized(&appended))
                .await;
        result.unwrap();

        let transition = &lifecycle.transitions[0];
        assert_eq!(transition.before.run_slices, step_count);
        assert_eq!(transition.before.steps, step_count);
        assert_eq!(transition.before.outcome_messages, step_count);
        assert_eq!(transition.after.run_slices, step_count + 1);
        assert_eq!(transition.after.steps, step_count + 1);
        assert_eq!(transition.after.outcome_messages, step_count + 1);
        assert!(transition.replaced_generation.upgrade().is_none());
    }
}

#[tokio::test]
async fn accepted_input_persists_before_publish() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer.clone());
    let accepted = accepted_input("input-v1");

    let receipt = repository.append_accepted_input(&accepted).await.unwrap();

    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    assert_eq!(writer.saved.lock().unwrap().len(), 1);
    {
        let session = holder.read().unwrap();
        let step = &session.run_slices[0].steps[0];
        assert_eq!(
            step.accepted_input.as_ref().unwrap().messages[0].text_content(),
            "accepted fact"
        );
        assert!(step.outcome.is_none());
    }
    assert_eq!(
        repository
            .snapshot(&accepted.session_id)
            .await
            .unwrap()
            .messages[0]
            .text_content(),
        "accepted fact"
    );
}

#[tokio::test]
async fn accepted_input_is_idempotent_but_rejects_content_conflict() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, _) = repository(writer);
    let accepted = accepted_input("input-v1");

    let first = repository.append_accepted_input(&accepted).await.unwrap();
    let second = repository.append_accepted_input(&accepted).await.unwrap();
    assert_eq!(first, second);

    let mut conflicting = accepted;
    conflicting.fingerprint = ContentFingerprint::new("input-v2");
    assert!(matches!(
        repository.append_accepted_input(&conflicting).await,
        Err(AcceptedInputError::ContentConflict { .. })
    ));
}

#[tokio::test]
async fn finalized_append_bridges_messages_into_structured_outcome() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let run_id = RunId::new("run");
    let step_id = RunStepId::new("step");
    let mut finalized = append("same");
    finalized.run_id = run_id.clone();
    finalized.step_id = step_id.clone();

    repository.append_finalized(&finalized).await.unwrap();

    let session = holder.read().unwrap();
    assert_eq!(session.run_slices.len(), 1);
    assert_eq!(session.run_slices[0].run_id, run_id.to_string());
    assert_eq!(session.run_slices[0].steps.len(), 1);
    assert_eq!(session.run_slices[0].steps[0].step_id, step_id.as_str());
    let outcome = session.run_slices[0].steps[0].outcome.as_ref().unwrap();
    assert_eq!(outcome.finalize_cause, FinalizeCause::Completed);
    assert_eq!(outcome.messages[0].text_content(), "fact");
    assert!(outcome.receipts.is_empty());
    assert_eq!(outcome.api_input_tokens, None);
    assert_eq!(outcome.fingerprint, "same");
    assert_eq!(outcome.committed_revision, 1);
}

#[tokio::test]
async fn finalized_outcome_preserves_accepted_input_and_receipt_metadata() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);
    let accepted = accepted_input("input-v1");
    repository.append_accepted_input(&accepted).await.unwrap();

    let mut finalized = append("outcome-v1");
    finalized.expected_revision = SessionRevision::new(1);
    finalized.finalize_cause = FinalizeCause::UserCancelledStep;
    finalized.api_input_tokens = Some(42);
    finalized.receipts = vec![context::domain::StepReceipt::agent(
        "agent-call",
        0,
        context::domain::ToolOutcomeKind::CancellationUnconfirmed,
    )];
    let receipt = repository.append_finalized(&finalized).await.unwrap();

    let session = holder.read().unwrap();
    let step = &session.run_slices[0].steps[0];
    assert_eq!(
        step.accepted_input.as_ref().unwrap().messages[0].text_content(),
        "accepted fact"
    );
    assert_eq!(
        step.accepted_input.as_ref().unwrap().fingerprint,
        "input-v1"
    );
    assert_eq!(step.accepted_input.as_ref().unwrap().committed_revision, 1);
    let outcome = step.outcome.as_ref().unwrap();
    assert_eq!(outcome.finalize_cause, FinalizeCause::UserCancelledStep);
    assert_eq!(outcome.api_input_tokens, Some(42));
    assert_eq!(
        outcome.receipts[0].outcome(),
        context::domain::ToolOutcomeKind::CancellationUnconfirmed
    );
    assert_eq!(outcome.fingerprint, "outcome-v1");
    assert_eq!(outcome.committed_revision, receipt.committed_revision.get());
}

#[tokio::test]
async fn snapshot_shares_committed_step_message_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let session = ten_step_session(&session_id, vec![], 10);
    let original_ptr = session.run_slices[0].steps[0]
        .outcome
        .as_ref()
        .unwrap()
        .messages
        .as_ptr();
    let (repository, _) = repository_with_session(writer, session);

    let snapshot = repository.snapshot(&session_id).await.unwrap();

    assert_eq!(snapshot.messages.len(), 10);
    assert_eq!(
        snapshot.messages.first().map(|message| message as *const _),
        Some(original_ptr)
    );
}

#[tokio::test]
async fn repeated_snapshots_share_every_committed_step_message_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let session = ten_step_session(&session_id, vec![], 10);
    let original_ptrs = session
        .run_slices
        .iter()
        .map(|slice| slice.steps[0].outcome.as_ref().unwrap().messages.as_ptr())
        .collect::<Vec<_>>();
    let (repository, _) = repository_with_session(writer, session);

    let first = repository.snapshot(&session_id).await.unwrap();
    let second = repository.snapshot(&session_id).await.unwrap();

    let first_ptrs = first
        .messages
        .iter()
        .map(|message| message as *const _)
        .collect::<Vec<_>>();
    let second_ptrs = second
        .messages
        .iter()
        .map(|message| message as *const _)
        .collect::<Vec<_>>();
    assert_eq!(first_ptrs, original_ptrs);
    assert_eq!(second_ptrs, original_ptrs);
}

#[tokio::test]
async fn snapshot_after_compact_shares_only_visible_step_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("compact-shared-session");
    let session = ten_step_session(&session_id, vec![], 0);
    let all_ptrs = session
        .run_slices
        .iter()
        .map(|slice| slice.steps[0].outcome.as_ref().unwrap().messages.as_ptr())
        .collect::<Vec<_>>();
    let (repository, _) = repository_with_session(writer, session);
    compact(&repository, session_id.clone(), 0).await;

    let snapshot = repository.snapshot(&session_id).await.unwrap();
    let visible_ptrs = snapshot
        .messages
        .iter()
        .map(|message| message as *const _)
        .collect::<Vec<_>>();

    assert!(!visible_ptrs.is_empty());
    assert!(visible_ptrs.len() < all_ptrs.len());
    assert_eq!(
        visible_ptrs,
        all_ptrs[all_ptrs.len() - visible_ptrs.len()..]
    );
}

#[tokio::test]
async fn snapshot_reads_structured_projection_not_legacy_chats() {
    let writer = Arc::new(RecordingWriter::default());
    let mut legacy = ChatSegment::normal(None);
    legacy.messages = vec![Message::user("legacy-only")];
    let session_id = SessionId::new("session");
    let session = CanonicalSession {
        id: session_id.to_string(),
        chats: vec![legacy],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(workspace()),
        revision: 0,
        compact: None,
        run_slices: vec![CommittedRunSlice::new(
            "run",
            vec![CommittedRunStep::accepted_only(
                "step",
                AcceptedInputProjection::new(vec![Message::user("structured-only")], "fp", 0),
            )],
        )]
        .into(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    };
    let (repository, _) = repository_with_session(writer, session);

    let snapshot = repository.snapshot(&session_id).await.unwrap();
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].text_content(), "structured-only");
    assert!(snapshot.active_summary.is_none());
}

#[tokio::test]
async fn finalized_append_reuses_existing_committed_step_entry_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("shared-ledger");
    let mut session = CanonicalSession::fixture(session_id.as_str());
    session.revision = 1;
    session.committed_steps = vec![context::domain::session::CommittedStep::fixture(
        "run-existing",
        "step-existing",
        "existing",
        1,
    )]
    .into();
    let existing_entry = Arc::clone(&session.committed_steps.entries()[0]);
    let (repository, holder) = repository_with_session(writer, session);
    let mut mutation = append("new");
    mutation.session_id = session_id;
    mutation.expected_revision = SessionRevision::new(1);
    mutation.run_id = RunId::new("run-new");
    mutation.step_id = RunStepId::new("step-new");

    repository
        .append_finalized(&mutation)
        .await
        .expect("append finalized outcome");

    let committed = holder.read().unwrap();
    assert!(Arc::ptr_eq(
        &existing_entry,
        &committed.committed_steps.entries()[0]
    ));
    assert_eq!(committed.committed_steps.len(), 2);
}

#[tokio::test]
async fn finalized_append_reuses_unchanged_run_slice_backing() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("shared-history");
    let session = CanonicalSession {
        id: session_id.to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(workspace()),
        revision: 1,
        compact: None,
        run_slices: vec![CommittedRunSlice::new(
            "run-existing",
            vec![CommittedRunStep::accepted_only(
                "step-existing",
                AcceptedInputProjection::new(vec![Message::user("existing")], "existing", 1),
            )],
        )]
        .into(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    };
    let existing_slice = Arc::clone(&session.run_slices.slices()[0]);
    let (repository, holder) = repository_with_session(writer, session);
    let mut mutation = append("new");
    mutation.session_id = session_id;
    mutation.expected_revision = SessionRevision::new(1);
    mutation.run_id = RunId::new("run-new");
    mutation.step_id = RunStepId::new("step-new");

    repository
        .append_finalized(&mutation)
        .await
        .expect("append finalized outcome");

    let committed = holder.read().unwrap();
    assert!(Arc::ptr_eq(
        &existing_slice,
        &committed.run_slices.slices()[0]
    ));
    assert_eq!(committed.run_slices.len(), 2);
}

#[tokio::test]
async fn append_persists_candidate_before_publishing_revision() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer.clone());

    let receipt = repository.append_finalized(&append("same")).await.unwrap();

    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(writer.saved.lock().unwrap().len(), 1);
    assert!(matches!(
        holder.read().unwrap().tasks,
        SnapshotState::Captured(_)
    ));
}

#[tokio::test]
async fn advance_tool_receipt_persists_before_publish_and_is_idempotent() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer.clone());
    let mutation =
        ToolReceiptMutation::pending(tool_identity(), "{\"pattern\":\"**/archify.mjs\"}");

    let first = repository
        .advance_tool_receipt(mutation.clone())
        .await
        .unwrap();
    let second = repository.advance_tool_receipt(mutation).await.unwrap();

    assert!(first.changed);
    assert!(!second.changed);
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(
        writer.saved.lock().unwrap().len(),
        1,
        "Tool receipt 更新应通过增量 Session writer 提交 before/after generation"
    );
    assert_eq!(
        holder.read().unwrap().run_slices[0].steps[0].tool_receipts[0].state,
        ToolCallState::Pending
    );
}

#[tokio::test]
async fn advance_tool_receipt_write_failure_does_not_publish_candidate() {
    let writer = Arc::new(RecordingWriter {
        saved: Mutex::new(vec![]),
        fail: true,
    });
    let (repository, holder) = repository(writer);

    assert!(matches!(
        repository
            .advance_tool_receipt(ToolReceiptMutation::pending(tool_identity(), "safe preview"))
            .await,
        Err(context::domain::ToolReceiptMutationError::Storage(message)) if message == "disk full"
    ));
    assert_eq!(holder.read().unwrap().revision, 0);
    assert!(holder.read().unwrap().run_slices.is_empty());
}

#[tokio::test]
async fn concurrent_appends_with_same_revision_allow_one_commit_and_reject_one_cas() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, holder) = repository(writer);

    let mut first = append("first");
    first.run_id = RunId::new("run-first");
    first.step_id = RunStepId::new("step-first");
    let mut second = append("second");
    second.run_id = RunId::new("run-second");
    second.step_id = RunStepId::new("step-second");

    let (first_result, second_result) = tokio::join!(
        repository.append_finalized(&first),
        repository.append_finalized(&second)
    );
    let results = [first_result, second_result];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ContextAppendError::RevisionConflict { .. })))
            .count(),
        1
    );
    assert_eq!(holder.read().unwrap().revision, 1);
    assert_eq!(holder.read().unwrap().committed_steps.len(), 1);
}

#[tokio::test]
async fn failed_durable_write_does_not_publish_candidate() {
    let writer = Arc::new(RecordingWriter {
        saved: Mutex::new(vec![]),
        fail: true,
    });
    let (repository, holder) = repository(writer);

    assert!(matches!(
        repository.append_finalized(&append("same")).await,
        Err(ContextAppendError::Storage(message)) if message == "disk full"
    ));
    assert_eq!(holder.read().unwrap().revision, 0);
}

#[tokio::test]
async fn automatic_compaction_executes_after_actual_token_decision() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("actual-token-session");
    let (repository, _) = repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    let mut request = compact_request(session_id);
    request.context_size = 1_000_000;
    request.max_output_tokens = 8_192;
    request.last_api_total_tokens = Some(900_000);

    let outcome = repository
        .commit_compaction(&CompactRequest {
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: CompactTrigger::Automatic,
        })
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        context::domain::CompactOutcome::Committed(_)
    ));
}

#[tokio::test]
async fn manual_compaction_bypasses_automatic_threshold() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("manual-compact-session");
    let (repository, _) = repository_with_session(writer, ten_step_session(&session_id, vec![], 0));

    let outcome = repository
        .commit_manual_compaction(&ManualCompactRequest {
            session_id,
            run_id: RunId::new("manual-run"),
            system_prompt: SystemPromptSpec::new("system"),
            context_size: 1_000_000,
        })
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        context::domain::CompactOutcome::Committed(_)
    ));
}

#[tokio::test]
async fn compact_marker_keeps_new_steps_visible() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("marker-session");
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    compact(&repository, session_id.clone(), 0).await;

    let mut appended = append("new-step");
    appended.session_id = session_id;
    appended.expected_revision = SessionRevision::new(1);
    appended.run_id = RunId::new("run-new");
    appended.step_id = RunStepId::new("step-new");
    appended.messages = vec![Message::user("newly-visible")];
    repository.append_finalized(&appended).await.unwrap();

    let session = holder.read().unwrap();
    assert!(session
        .compact
        .as_ref()
        .and_then(|marker| marker.start_at.as_ref())
        .is_some());
    assert!(session
        .structured_messages()
        .iter()
        .any(|message| message.text_content() == "newly-visible"));
}

#[tokio::test]
async fn second_compact_advances_single_marker() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("second-marker-session");
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 0));
    compact(&repository, session_id.clone(), 0).await;
    let first = holder
        .read()
        .unwrap()
        .compact
        .as_ref()
        .unwrap()
        .start_at
        .clone()
        .unwrap();

    for index in 0..6 {
        let mut appended = append(format!("new-step-{index}").as_str());
        appended.session_id = session_id.clone();
        appended.expected_revision = SessionRevision::new(1 + index);
        appended.run_id = RunId::new(format!("run-new-{index}"));
        appended.step_id = RunStepId::new(format!("step-new-{index}"));
        appended.messages = vec![Message::user(format!("newly-visible-{index}"))];
        repository.append_finalized(&appended).await.unwrap();
    }
    compact(&repository, session_id, 7).await;

    let session = holder.read().unwrap();
    let marker = session.compact.as_ref().unwrap();
    assert_ne!(marker.start_at.as_ref(), Some(&first));
    assert!(marker.summary.contains("Previous compact summary"));
    assert!(session
        .structured_messages()
        .iter()
        .any(|message| message.text_content() == "newly-visible-5"));
}

#[tokio::test]
async fn compaction_rejects_stale_source_revision() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let (repository, holder) =
        repository_with_session(writer, ten_step_session(&session_id, vec![], 2));
    let request = compact_request(session_id);

    let result = repository
        .commit_compaction(&CompactRequest {
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(1),
            source: request,
            trigger: CompactTrigger::Automatic,
        })
        .await;

    assert!(matches!(
        result,
        Err(context::domain::ContextPortError::Compact(_))
    ));
    assert_eq!(holder.read().unwrap().revision, 2);
}

#[tokio::test]
async fn duplicate_key_is_idempotent_and_conflicting_content_is_typed() {
    let writer = Arc::new(RecordingWriter::default());
    let (repository, _) = repository(writer.clone());

    let first = repository.append_finalized(&append("same")).await.unwrap();
    let second = repository.append_finalized(&append("same")).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(writer.saved.lock().unwrap().len(), 1);
    assert!(matches!(
        repository.append_finalized(&append("different")).await,
        Err(ContextAppendError::ContentConflict { .. })
    ));
}

/// #1486：注入 generator 后 commit_compaction 走 LLM 语义压缩，
/// summary 来自生成器而非本地 fallback 模板。
#[tokio::test]
async fn commit_compaction_with_generator_uses_llm_summary() {
    use context::compact::CompactGenerator;
    use tokio_util::sync::CancellationToken;

    struct FixedGenerator(&'static str);
    #[async_trait::async_trait]
    impl CompactGenerator for FixedGenerator {
        async fn generate(
            &self,
            _request: Vec<Message>,
            _cancel: &CancellationToken,
        ) -> Result<String, String> {
            Ok(format!("<summary>{}</summary>", self.0))
        }
    }

    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let (base_repository, _holder) =
        repository_with_session(writer.clone(), ten_step_session(&session_id, vec![], 0));
    let repository_under_test =
        base_repository.with_generator(Arc::new(FixedGenerator("LLM 生成的语义摘要")));

    compact(&repository_under_test, session_id.clone(), 0).await;

    let saved = writer.saved.lock().unwrap().last().cloned().unwrap();
    let summary = saved.compact.expect("compact marker 应已写入").summary;
    assert!(
        summary.contains("LLM 生成的语义摘要"),
        "summary 应来自 LLM 生成器: {summary}"
    );
    assert!(
        !summary.contains("## User Requests"),
        "不应使用本地 fallback 模板: {summary}"
    );
}
