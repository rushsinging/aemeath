use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use context::adapters::{CanonicalSessionRepository, CanonicalSessionWriter};
use context::domain::session::{
    AcceptedInputProjection, CanonicalSession, ChatSegment, CommittedRunSlice, CommittedRunStep,
    SnapshotState,
};
use context::domain::{
    AcceptedInputAppend, AcceptedInputError, CalendarDate, CompactRequest, CompactTrigger,
    ContentFingerprint, ContextAppend, ContextAppendError, ContextRequest, ContextRequestId,
    FinalizeCause, Language, RunStepId, SessionId, SessionRevision, SystemPromptSpec,
    TaskReminderSnapshot,
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

#[derive(Default)]
struct RecordingWriter {
    saved: Mutex<Vec<CanonicalSession>>,
    fail: bool,
}

#[async_trait]
impl CanonicalSessionWriter for RecordingWriter {
    async fn save(&self, session: &CanonicalSession) -> Result<(), String> {
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
        current_date: CalendarDate::new("2026-07-19"),
        task_reminder: TaskReminderSnapshot::default(),
        language: Language::new("zh"),
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 1,
        max_output_tokens: 1,
        last_api_input_tokens: Some(100),
        tool_schemas: vec![],
        tool_schema_tokens: 0,
        prev_system_tokens: None,
        prev_tool_schema_tokens: None,
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
            run_slices: vec![],
            committed_steps: vec![],
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
        run_slices: ten_step_slices(),
        committed_steps: vec![],
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
        )],
        committed_steps: vec![],
    };
    let (repository, _) = repository_with_session(writer, session);

    let snapshot = repository.snapshot(&session_id).await.unwrap();
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].text_content(), "structured-only");
    assert!(snapshot.active_summary.is_none());
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
