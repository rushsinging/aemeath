use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use context::adapters::{CanonicalSessionRepository, CanonicalSessionWriter};
use context::domain::session::{CanonicalSession, ChatSegment, SnapshotState};
use context::domain::{
    CalendarDate, CompactRequest, CompactTrigger, ContentFingerprint, ContextAppend,
    ContextAppendError, ContextRequest, ContextRequestId, FinalizeCause, Language, RunStepId,
    SessionId, SessionRevision, SystemPromptSpec, TaskReminderSnapshot,
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
    let session_id = SessionId::new("session").to_string();
    repository_with_session(
        writer,
        CanonicalSession {
            id: session_id,
            chats: vec![],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            metadata: Default::default(),
            tasks: SnapshotState::Missing,
            workspace: SnapshotState::Captured(workspace()),
            revision: 0,
            committed_steps: vec![],
        },
    )
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
async fn compaction_appends_one_compact_segment_without_duplicating_active_history() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let mut segment = ChatSegment::normal(None);
    segment.messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect();
    let (repository, holder) = repository_with_session(
        writer,
        CanonicalSession {
            id: session_id.to_string(),
            chats: vec![segment],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            metadata: Default::default(),
            tasks: SnapshotState::Missing,
            workspace: SnapshotState::Captured(workspace()),
            revision: 0,
            committed_steps: vec![],
        },
    );
    let request = ContextRequest {
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
    };

    repository
        .commit_compaction(&CompactRequest {
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(0),
            source: request,
            trigger: CompactTrigger::Automatic,
        })
        .await
        .unwrap();

    let session = holder.read().unwrap();
    assert_eq!(session.chats.len(), 2);
    assert!(session.chats[1].summary.is_some());
}

#[tokio::test]
async fn compaction_rejects_stale_source_revision() {
    let writer = Arc::new(RecordingWriter::default());
    let session_id = SessionId::new("session");
    let mut segment = ChatSegment::normal(None);
    segment.messages = (0..10)
        .map(|index| Message::user(format!("message-{index}")))
        .collect();
    let (repository, holder) = repository_with_session(
        writer,
        CanonicalSession {
            id: session_id.to_string(),
            chats: vec![segment],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            metadata: Default::default(),
            tasks: SnapshotState::Missing,
            workspace: SnapshotState::Captured(workspace()),
            revision: 2,
            committed_steps: vec![],
        },
    );
    let request = ContextRequest {
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
    };

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
