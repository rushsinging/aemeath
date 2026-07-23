//! Integration tests for `MainSessionWiring` — the Context-owned coordinator
//! that binds Main Run to a consistent Session/Memory/Config triple and
//! atomically resumes from a prepared `CanonicalSession`.
//!
//! These tests use **real** `project` workspace and `task::TaskStore` backing
//! (they are in-memory / filesystem-light) and **real** `ConfigAppService`.
//! Only the `MemoryOpener` is mocked.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use config::{ConfigAppService, ConfigReader, ProjectConfigParticipant};
use context::application::main_session::{
    MainSessionError, MainSessionWiring, MainSessionWiringBuilder,
};
use context::domain::session::{CanonicalSession, SnapshotState};
use context::domain::{
    AcceptedInputAppend, ContentFingerprint, ContextAppend, ContextRequestId, FinalizeCause,
    RunStepId, SessionId, SessionRevision, StepReceipt, ToolOutcomeKind,
};
use context::ports::ContextPort;
use memory::{
    InMemoryMemory, MemoryOpener, MemoryOpenerError, MemoryPolicy, MemoryPort, ProjectMemoryKey,
};
use project::wire_production_workspace;
use sdk::RunId;
use share::message::Message;
use share::session_types::PersistedWorkspaceContext;
use task::{
    BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPersist, TaskPriority, TaskSnapshot,
    TaskSnapshotValidationError, TaskStore,
};

// ─── RAII temp directory ─────────────────────────────────────────────

/// Shared base directory for all test temp dirs.  Setting `GIT_CEILING_DIRECTORIES`
/// to this path prevents `wire_production_workspace` from discovering a parent
/// git repo, so every temp dir is treated as `NonGit`.
fn ensure_ceiling() -> &'static str {
    static CEILING: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let val = CEILING.get_or_init(|| {
        let base = std::env::temp_dir();
        // Safe to set process-wide: every test temp dir lives under this base.
        // git will never traverse above it.
        let path = base.display().to_string();
        // Append the existing value if present.
        let existing = std::env::var("GIT_CEILING_DIRECTORIES").unwrap_or_default();
        let combined = if existing.is_empty() {
            path.clone()
        } else {
            format!("{path}:{existing}")
        };
        // SAFETY: tests are single-binary; the env var only affects child git
        // processes spawned by `wire_production_workspace`.  All test threads
        // want the same ceiling behaviour.
        unsafe { std::env::set_var("GIT_CEILING_DIRECTORIES", &combined) };
        path
    });
    val.as_str()
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let _ = ensure_ceiling();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aemeath-main-session-{tag}-{nanos}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self {
            path: path.canonicalize().unwrap(),
        }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ─── Test serialization ──────────────────────────────────────────────
//
// `wire_production_workspace` and `WorkspacePersist::prepare_restore` both
// spawn `git` subprocesses.  Under heavy parallel test execution the OS can
// intermittently fail to spawn the subprocess (`ErrorKind::NotFound`),
// producing `GitProbeFailed(GitUnavailable)`.  We serialize all tests that
// touch the real workspace through a global async mutex.

static GIT_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Acquires the global serialization lock.  Drop the guard to release.
async fn git_lock() -> tokio::sync::MutexGuard<'static, ()> {
    GIT_GUARD.lock().await
}

// ─── Mock MemoryOpener ───────────────────────────────────────────────

struct MockMemoryOpener {
    open_count: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
}

impl MockMemoryOpener {
    fn new() -> Self {
        Self {
            open_count: Arc::new(AtomicUsize::new(0)),
            fail: Arc::new(AtomicBool::new(false)),
        }
    }

    fn open_count(&self) -> usize {
        self.open_count.load(Ordering::SeqCst)
    }

    fn set_fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }
}

#[async_trait]
impl MemoryOpener for MockMemoryOpener {
    async fn open_memory(
        &self,
        _key: &ProjectMemoryKey,
        _config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn MemoryPort>, MemoryOpenerError> {
        self.open_count.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(MemoryOpenerError::Io);
        }
        let policy = MemoryPolicy::default();
        Ok(Arc::new(InMemoryMemory::new(policy).unwrap()) as Arc<dyn MemoryPort>)
    }

    fn boxed_clone(&self) -> Box<dyn MemoryOpener> {
        Box::new(MockMemoryOpener {
            open_count: Arc::clone(&self.open_count),
            fail: Arc::clone(&self.fail),
        })
    }
}

// ─── Test harness ────────────────────────────────────────────────────

struct Harness {
    wiring: MainSessionWiring,
    workspace_persist: Arc<dyn project::WorkspacePersist>,
    task_store: Arc<TaskStore>,
    task_access: Arc<dyn TaskAccess>,
    config_service: Arc<ConfigAppService>,
    memory_opener: Arc<MockMemoryOpener>,
    _tmp: TempDir,
}

fn build_harness() -> Harness {
    let tmp = TempDir::new("harness");

    // Real workspace (non-git).
    let workspace = wire_production_workspace(tmp.path().to_path_buf()).unwrap();
    let workspace_read = workspace.read();
    let workspace_persist = workspace.persist();

    // Real task store.
    let task_store = Arc::new(TaskStore::new());
    let task_access: Arc<dyn TaskAccess> = task_store.clone();
    let task_persist: Arc<dyn task::TaskPersist> = task_store.clone();

    // Real config service (no project dir, dummy global path).
    let config_service = Arc::new(ConfigAppService::with_global_path(
        None,
        tmp.path().join("nonexistent_global.json"),
    ));
    let config_reader: Arc<dyn ConfigReader> = config_service.clone();
    let config_participant: Arc<dyn ProjectConfigParticipant> = config_service.clone();

    // Mock memory opener.
    let memory_opener = Arc::new(MockMemoryOpener::new());

    // Initial memory port.
    let initial_memory: Arc<dyn MemoryPort> =
        Arc::new(InMemoryMemory::new(MemoryPolicy::default()).unwrap());

    // Capture workspace snapshot for the initial session.
    let ws_ctx = workspace_persist.snapshot();
    let initial_session = CanonicalSession {
        id: SessionId::new("initial").to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(ws_ctx),
        revision: 0,
        compact: None,
        run_slices: vec![],
        committed_steps: vec![],
    };

    let builder = MainSessionWiringBuilder {
        workspace_read,
        workspace_persist: workspace_persist.clone(),
        task_persist,
        config_reader,
        config_participant,
        memory_opener: Box::new(MockMemoryOpener {
            open_count: Arc::clone(&memory_opener.open_count),
            fail: Arc::clone(&memory_opener.fail),
        }),
        session_management: Arc::new(context::adapters::AtomicBlobSessionManagement::new(
            Arc::new(storage::FileSystemBlobAdapter::new(tmp.path()).unwrap()),
        )),
        initial_session,
        initial_memory,
        context_factory: Arc::new(context::adapters::ProductionMainContextFactory::new(
            Arc::new(context::adapters::NoOpCanonicalSessionWriter),
        )),
    };

    let wiring = MainSessionWiring::build(builder);

    Harness {
        wiring,
        workspace_persist,
        task_store,
        task_access,
        config_service,
        memory_opener,
        _tmp: tmp,
    }
}

/// Builds a `CanonicalSession` whose workspace matches the harness's live workspace.
fn session_with_workspace(
    ws: &PersistedWorkspaceContext,
    tasks: SnapshotState<TaskSnapshot>,
) -> CanonicalSession {
    CanonicalSession {
        id: "resume-target".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks,
        workspace: SnapshotState::Captured(ws.clone()),
        revision: 1,
        compact: None,
        run_slices: vec![],
        committed_steps: vec![],
    }
}

/// Seeds `count` tasks into the task store (all in a single batch).
fn seed_tasks(access: &dyn TaskAccess, count: usize) {
    let batch = BatchCreateSpec::try_new("seed-batch".to_owned()).unwrap();
    access.create_batch(batch, 0).unwrap();
    for i in 0..count {
        let spec = TaskCreateSpec::try_new(
            format!("task-{i}"),
            String::new(),
            None,
            TaskPriority::Normal,
        )
        .unwrap();
        access.create_task(spec, (i + 1) as u64).unwrap();
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

/// A successful resume commits every participant and updates the committed
/// Session/Memory/Config so that `bind_main_run` returns the new state.
#[tokio::test]
async fn successful_resume_commits_all_and_updates_committed_state() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Capture pre-resume state.
    let pre_session_id = h.wiring.committed_session().id.clone();
    let pre_memory = h.wiring.committed_memory();
    let pre_config_revision = h.config_service.committed_snapshot().revision();

    // Build a resume session with the current workspace.
    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Captured(TaskSnapshot::empty()));

    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume should succeed");

    // Verify committed session changed.
    let post_session = h.wiring.committed_session();
    assert_eq!(
        post_session.id, "resume-target",
        "committed session should be the resumed one"
    );
    assert_ne!(post_session.id, pre_session_id);

    // Verify committed memory changed (new Arc).
    let post_memory = h.wiring.committed_memory();
    assert!(
        !Arc::ptr_eq(&pre_memory, &post_memory),
        "committed memory should be a new Arc after resume"
    );

    // Verify config was committed (revision should advance).
    let post_config_revision = h.config_service.committed_snapshot().revision();
    assert_ne!(
        post_config_revision, pre_config_revision,
        "config revision should advance after commit_project"
    );

    // Verify memory opener was called exactly once.
    assert_eq!(
        h.memory_opener.open_count(),
        1,
        "memory opener should be called once"
    );

    // Verify bind_main_run returns the new state.
    let bound = h.wiring.bind_main_run().await.expect("bind should succeed");
    assert_eq!(bound.session().id, "resume-target");
    assert_eq!(bound.config().revision(), post_config_revision);
}

#[tokio::test]
async fn accepted_input_persists_and_is_visible_after_resume_without_outcome() {
    let _guard = git_lock().await;
    let h = build_harness();
    let bound = h.wiring.bind_main_run().await.expect("bind source run");
    let context: Arc<dyn ContextPort> = bound.context();
    let source_id = bound.session().id.clone();
    let workspace = bound.session().workspace.clone();
    drop(bound);

    let receipt = context
        .append_accepted_input(&AcceptedInputAppend {
            session_id: SessionId::new(source_id.clone()),
            run_id: RunId::new("run-accepted"),
            step_id: RunStepId::new("step-accepted"),
            source_request_id: ContextRequestId::new("request-accepted"),
            messages: vec![Message::user("accepted before model")],
            fingerprint: ContentFingerprint::new("accepted-fingerprint"),
        })
        .await
        .expect("append accepted input");
    assert_eq!(receipt.committed_revision, SessionRevision::new(1));

    let committed = h.wiring.committed_session();
    let step = &committed.run_slices[0].steps[0];
    assert!(step.outcome.is_none());
    assert_eq!(
        step.accepted_input.as_ref().unwrap().messages[0].text_content(),
        "accepted before model"
    );

    let mut resumed = (*committed).clone();
    resumed.workspace = workspace;
    h.wiring
        .resume_prepared(resumed)
        .await
        .expect("resume accepted-only session");
    let rebound = h.wiring.bind_main_run().await.expect("bind resumed run");
    let resumed_step = &rebound.session().run_slices[0].steps[0];
    assert!(resumed_step.outcome.is_none());
    assert_eq!(
        rebound.session().structured_messages()[0].text_content(),
        "accepted before model"
    );
}

#[tokio::test]
async fn finalized_outcome_metadata_survives_resume_without_runtime_state() {
    let _guard = git_lock().await;
    let h = build_harness();
    let bound = h.wiring.bind_main_run().await.expect("bind source run");
    let context: Arc<dyn ContextPort> = bound.context();
    let source_id = bound.session().id.clone();
    let workspace = bound.session().workspace.clone();
    drop(bound);

    context
        .append_accepted_input(&AcceptedInputAppend {
            session_id: SessionId::new(source_id.clone()),
            run_id: RunId::new("run-finalized"),
            step_id: RunStepId::new("step-finalized"),
            source_request_id: ContextRequestId::new("request-finalized"),
            messages: vec![Message::user("accepted input")],
            fingerprint: ContentFingerprint::new("input-fingerprint"),
        })
        .await
        .expect("append accepted input");
    context
        .append_and_persist(&ContextAppend {
            session_id: SessionId::new(source_id.clone()),
            expected_revision: SessionRevision::new(1),
            run_id: RunId::new("run-finalized"),
            step_id: RunStepId::new("step-finalized"),
            source_request_id: ContextRequestId::new("request-finalized"),
            finalize_cause: FinalizeCause::RunTerminated,
            messages: vec![Message::user("finalized partial")],
            receipts: vec![StepReceipt::agent(
                "agent-call",
                0,
                ToolOutcomeKind::CancellationUnconfirmed,
            )],
            api_input_tokens: Some(34),
            fingerprint: ContentFingerprint::new("outcome-fingerprint"),
        })
        .await
        .expect("append finalized outcome");

    let mut resumed = (*h.wiring.committed_session()).clone();
    resumed.workspace = workspace;
    h.wiring
        .resume_prepared(resumed)
        .await
        .expect("resume finalized session");
    let rebound = h.wiring.bind_main_run().await.expect("bind resumed run");
    let step = &rebound.session().run_slices[0].steps[0];
    assert_eq!(
        rebound
            .session()
            .structured_messages()
            .iter()
            .map(|message| message.text_content())
            .collect::<Vec<_>>(),
        ["accepted input", "finalized partial"]
    );
    let outcome = step.outcome.as_ref().expect("finalized outcome");
    assert_eq!(outcome.finalize_cause, FinalizeCause::RunTerminated);
    assert_eq!(outcome.api_input_tokens, Some(34));
    assert_eq!(
        outcome.receipts[0].outcome(),
        ToolOutcomeKind::CancellationUnconfirmed
    );
    assert_eq!(outcome.committed_revision, 2);
}

#[tokio::test]
async fn finalized_append_persists_and_is_visible_after_resume() {
    let _guard = git_lock().await;
    let h = build_harness();
    let bound = h.wiring.bind_main_run().await.expect("bind source run");
    let context: Arc<dyn ContextPort> = bound.context();
    let source_id = bound.session().id.clone();
    let workspace = bound.session().workspace.clone();
    drop(bound);

    let append = ContextAppend {
        session_id: SessionId::new(source_id.clone()),
        expected_revision: SessionRevision::new(0),
        run_id: RunId::new("run-persist"),
        step_id: RunStepId::new("step-persist"),
        source_request_id: ContextRequestId::new("request-persist"),
        finalize_cause: FinalizeCause::Completed,
        messages: vec![Message::user("durable fact")],
        receipts: vec![],
        api_input_tokens: Some(21),
        fingerprint: ContentFingerprint::new("durable-fingerprint"),
    };

    let receipt = context
        .append_and_persist(&append)
        .await
        .expect("append finalized step");
    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    let committed = h.wiring.committed_session();
    assert_eq!(committed.revision, 1);
    assert_eq!(committed.committed_steps.len(), 1);
    assert_eq!(
        committed.structured_messages().len(),
        1,
        "committed history must contain the finalized message"
    );

    let mut resumed = (*committed).clone();
    resumed.workspace = workspace;
    h.wiring
        .resume_prepared(resumed)
        .await
        .expect("resume committed session");
    let rebound = h.wiring.bind_main_run().await.expect("bind resumed run");
    assert_eq!(rebound.session().id, source_id);
    assert_eq!(rebound.session().revision, 1);
    assert_eq!(
        rebound.session().committed_steps[0].step_id,
        RunStepId::new("step-persist").to_string()
    );
    assert_eq!(rebound.session().structured_messages().len(), 1);
}

/// Cross-project resume is rejected before Config, Memory, Task or workspace
/// state can switch. The current project's committed resources remain intact.
#[tokio::test]
async fn cross_project_resume_is_rejected() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Capture pre-resume committed state.
    let pre_session_id = h.wiring.committed_session().id.clone();
    let pre_memory = h.wiring.committed_memory();
    let pre_config_revision = h.config_service.committed_snapshot().revision();

    // Build a session from a *different* project's workspace.
    let tmp2 = TempDir::new("cross-project");
    let ws2 = project::wire_production_workspace(tmp2.path().to_path_buf())
        .unwrap()
        .persist()
        .snapshot();
    let session = session_with_workspace(&ws2, SnapshotState::Captured(TaskSnapshot::empty()));

    // Resume must reject before any participant is prepared or committed.
    assert!(matches!(
        h.wiring.resume_prepared(session).await,
        Err(MainSessionError::ProjectMismatch)
    ));

    // Committed session remains unchanged.
    assert_eq!(
        h.wiring.committed_session().id,
        pre_session_id,
        "committed session must remain unchanged after cross-project rejection"
    );

    // Committed memory and Config remain unchanged.
    let target_memory = h.wiring.committed_memory();
    assert!(
        Arc::ptr_eq(&target_memory, &pre_memory),
        "committed memory must remain unchanged after cross-project rejection"
    );
    let bound = h.wiring.bind_main_run().await.expect("bind current run");
    assert!(
        std::ptr::eq(bound.memory(), pre_memory.as_ref()),
        "bound run should retain the current project's committed Memory Arc"
    );
    assert_eq!(bound.session().id, pre_session_id);
    assert_eq!(
        h.config_service.committed_snapshot().revision(),
        pre_config_revision,
        "config revision must remain unchanged after cross-project rejection"
    );
    assert_eq!(
        h.memory_opener.open_count(),
        0,
        "memory opener must not run for a rejected cross-project session"
    );
}

/// Resuming with a captured non-empty task snapshot installs it into the
/// authoritative TaskAccess view after all participants prepare successfully.
#[tokio::test]
async fn task_captured_snapshot_restores_tasks_into_task_access() {
    let _guard = git_lock().await;
    let source = build_harness();
    seed_tasks(&*source.task_access, 2);
    let captured = source.task_store.collect_snapshot();

    let target = build_harness();
    let ws = target.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Captured(captured.clone()));

    target
        .wiring
        .resume_prepared(session)
        .await
        .expect("captured task snapshot should resume");

    assert_eq!(target.task_store.collect_snapshot(), captured);
    assert_eq!(target.task_access.list().len(), 2);
}

#[tokio::test]
async fn pending_task_with_legacy_started_at_restores_after_snapshot_normalization() {
    let _guard = git_lock().await;
    let h = build_harness();
    let snapshot = TaskSnapshot::decode(
        br#"{"schema_version":2,"revision":"1","tasks":[{"id":"35","batch":"8","subject":"resumable","description":"","active_form":null,"session_id":null,"tags":[],"blocked_by":[],"status":"pending","priority":"high","created_at":100,"updated_at":300,"started_at":200,"completed_at":null}],"next_task_id":"36","next_batch_id":"9","current_batch":"8","batches":[{"id":"8","summary":"active","status":"active","created_at":100,"last_active_turn":0,"silence_turns":0}]}"#,
    )
    .expect("legacy pending task snapshot must decode");
    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Captured(snapshot));

    h.wiring
        .resume_prepared(session)
        .await
        .expect("normalized pending task snapshot must resume");

    let task = h
        .task_access
        .list()
        .into_iter()
        .next()
        .expect("task restored");
    assert_eq!(task.status(), task::TaskStatus::Pending);
}

/// A rejected Task snapshot must leave every already-live participant unchanged.
#[tokio::test]
async fn invalid_task_snapshot_keeps_committed_session_memory_and_tasks_unchanged() {
    let _guard = git_lock().await;
    let h = build_harness();
    seed_tasks(&*h.task_access, 1);
    let pre_session_id = h.wiring.committed_session().id.clone();
    let pre_memory = h.wiring.committed_memory();
    let before_tasks = h.task_store.collect_snapshot();
    let invalid = TaskSnapshot::decode(
        br#"{"schema_version":2,"revision":"1","tasks":[{"id":"1","batch":"1","subject":"t","description":"","active_form":null,"session_id":null,"tags":[],"blocked_by":["1"],"status":"pending","priority":"normal","created_at":1,"updated_at":1,"started_at":null,"completed_at":null}],"next_task_id":"2","next_batch_id":"2","current_batch":"1","batches":[{"id":"1","summary":"b","status":"active","created_at":1,"last_active_turn":0,"silence_turns":0}]}"#,
    )
    .expect("fixture must decode");
    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Captured(invalid));

    let result = h.wiring.resume_prepared(session).await;

    assert!(matches!(
        result,
        Err(MainSessionError::TaskRestore(
            TaskSnapshotValidationError::SelfDependency { .. }
        ))
    ));
    assert_eq!(h.wiring.committed_session().id, pre_session_id);
    assert!(Arc::ptr_eq(&h.wiring.committed_memory(), &pre_memory));
    assert_eq!(h.task_store.collect_snapshot(), before_tasks);
}

/// Resuming with `tasks: Missing` clears stale live tasks via
/// `TaskSnapshot::empty()`.
#[tokio::test]
async fn task_missing_clears_stale_live_tasks() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Seed tasks.
    seed_tasks(&*h.task_access, 3);

    // Resume with tasks: Missing.
    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Missing);

    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume should succeed");

    // Tasks should be cleared.
    let post = h.task_store.collect_snapshot();
    assert_eq!(
        post.tasks().len(),
        0,
        "Missing task slot should clear live tasks"
    );
}

/// Resuming with `tasks: CapturedEmpty` also clears stale live tasks.
#[tokio::test]
async fn task_captured_empty_clears_stale_live_tasks() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Seed tasks.
    seed_tasks(&*h.task_access, 1);

    // Resume with tasks: CapturedEmpty.
    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::CapturedEmpty);

    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume should succeed");

    assert_eq!(
        h.task_store.collect_snapshot().tasks().len(),
        0,
        "CapturedEmpty task slot should clear live tasks"
    );
}

/// 已删除的 worktree 仅降级到当前工作区，不能阻断会话内容恢复。
#[tokio::test]
async fn missing_persisted_workspace_falls_back_to_live_workspace_and_rewrites_snapshot() {
    let _guard = git_lock().await;
    let h = build_harness();
    let live_workspace = h.workspace_persist.snapshot();
    let mut stale_workspace = live_workspace.clone();
    let missing_root = h._tmp.path().join("deleted-worktree");
    stale_workspace.workspace_root = missing_root.display().to_string();
    stale_workspace.path_base = missing_root.display().to_string();

    let session = session_with_workspace(
        &stale_workspace,
        SnapshotState::Captured(TaskSnapshot::empty()),
    );

    h.wiring
        .resume_prepared(session)
        .await
        .expect("missing persisted workspace should fall back to live workspace");

    assert_eq!(
        h.workspace_persist.snapshot(),
        live_workspace,
        "live workspace must remain unchanged after fallback"
    );
    assert_eq!(
        h.wiring.committed_session().workspace,
        SnapshotState::Captured(live_workspace),
        "committed session must replace the stale workspace snapshot"
    );
}

#[tokio::test]
async fn missing_cross_project_workspace_remains_rejected() {
    let _guard = git_lock().await;
    let h = build_harness();
    let pre_session_id = h.wiring.committed_session().id.clone();
    let tmp2 = TempDir::new("missing-cross-project");
    let mut stale_workspace = project::wire_production_workspace(tmp2.path().to_path_buf())
        .unwrap()
        .persist()
        .snapshot();
    let missing_root = tmp2.path().join("deleted-worktree");
    stale_workspace.workspace_root = missing_root.display().to_string();
    stale_workspace.path_base = missing_root.display().to_string();
    let session = session_with_workspace(
        &stale_workspace,
        SnapshotState::Captured(TaskSnapshot::empty()),
    );

    let result = h.wiring.resume_prepared(session).await;
    assert!(
        matches!(result, Err(MainSessionError::ProjectMismatch)),
        "missing cross-project workspace must remain rejected, got {result:?}"
    );
    assert_eq!(h.wiring.committed_session().id, pre_session_id);
}

#[tokio::test]
async fn workspace_missing_returns_typed_error() {
    let _guard = git_lock().await;
    let h = build_harness();

    let pre_session_id = h.wiring.committed_session().id.clone();

    let session = CanonicalSession {
        id: "no-workspace".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Missing,
        revision: 1,
        compact: None,
        run_slices: vec![],
        committed_steps: vec![],
    };

    let result = h.wiring.resume_prepared(session).await;

    assert!(
        matches!(result, Err(MainSessionError::WorkspaceMissing)),
        "expected WorkspaceMissing, got {result:?}"
    );
    assert_eq!(
        h.wiring.committed_session().id,
        pre_session_id,
        "nothing should change on WorkspaceMissing"
    );
}

/// Resuming with `workspace: CapturedEmpty` also returns the typed error.
#[tokio::test]
async fn workspace_captured_empty_returns_typed_error() {
    let _guard = git_lock().await;
    let h = build_harness();

    let session = CanonicalSession {
        id: "empty-workspace".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::CapturedEmpty,
        revision: 1,
        compact: None,
        run_slices: vec![],
        committed_steps: vec![],
    };

    let result = h.wiring.resume_prepared(session).await;
    assert!(
        matches!(result, Err(MainSessionError::WorkspaceMissing)),
        "expected WorkspaceMissing for CapturedEmpty, got {result:?}"
    );
}

/// `bind_main_run` captures a consistent snapshot of the committed
/// Session/Memory/Config triple.  Multiple concurrent bindings see the same
/// Arc references as long as no resume intervenes.
#[tokio::test]
async fn bound_capture_is_consistent() {
    let _guard = git_lock().await;
    let h = build_harness();

    let bound1 = h.wiring.bind_main_run().await.expect("bind 1");
    let bound2 = h.wiring.bind_main_run().await.expect("bind 2");

    // Both bindings capture the same committed session Arc.
    assert!(
        Arc::ptr_eq(
            &Arc::new(bound1.session().clone()),
            &Arc::new(bound2.session().clone())
        ) || bound1.session() == bound2.session(),
        "both bindings should see the same committed session"
    );
    assert_eq!(
        bound1.session().id,
        bound2.session().id,
        "both bindings should see the same session id"
    );
    assert_eq!(
        bound1.config().revision(),
        bound2.config().revision(),
        "both bindings should see the same config revision"
    );

    // The shared permits block resume.
    let wiring = h.wiring.gate();
    assert!(
        wiring.try_acquire_shared().is_ok(),
        "additional shared permits should be allowed while bound runs exist"
    );

    drop(bound1);
    drop(bound2);

    // After dropping all bindings, resume can proceed.
    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Missing);
    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume after all bindings dropped");

    // New binding sees the updated session.
    let bound3 = h.wiring.bind_main_run().await.expect("bind 3");
    assert_eq!(bound3.session().id, "resume-target");
}

/// The exclusive permit held by `resume_prepared` blocks `bind_main_run`
/// until the resume completes.
#[tokio::test]
async fn resume_blocks_bind_until_complete() {
    let _guard = git_lock().await;
    let h = build_harness();

    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Missing);

    // Start resume and let it complete.
    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume should succeed");

    // After resume completes, bind should succeed immediately.
    let bound = h.wiring.bind_main_run().await.expect("bind after resume");
    assert_eq!(bound.session().id, "resume-target");
}

/// A memory opener failure (after Project/ACL/Config prepare succeeds) keeps
/// everything old.
#[tokio::test]
async fn memory_open_failure_keeps_all_old_state() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Seed a live task to verify it survives the failed resume.
    seed_tasks(&*h.task_access, 1);

    let pre_session_id = h.wiring.committed_session().id.clone();
    let pre_memory = h.wiring.committed_memory();

    // Force memory open to fail.
    h.memory_opener.set_fail(true);

    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Missing);

    let result = h.wiring.resume_prepared(session).await;
    assert!(
        matches!(result, Err(MainSessionError::MemoryOpen(_))),
        "expected MemoryOpen error, got {result:?}"
    );

    // Everything stays old.
    assert_eq!(h.wiring.committed_session().id, pre_session_id);
    assert!(Arc::ptr_eq(&h.wiring.committed_memory(), &pre_memory));
    assert_eq!(
        h.task_store.collect_snapshot().tasks().len(),
        1,
        "task must survive"
    );
}

// ─── Resume publishes only the canonical committed session ───────────
//
// `SessionProjectionParticipant` has been retired: `resume_prepared` no longer
// maintains a separate second projection backing.  The single source of truth
// is the canonical committed session held by `MainSessionWiring`.  These
// tests assert that contract directly:
//
//   * a successful resume publishes *only* the canonical committed session,
//   * a bound run created *after* resume observes that exact session.

/// `resume_prepared` publishes only the canonical committed session and a
/// `bind_main_run` issued afterwards observes it.  No second projection
/// backing is consulted.
#[tokio::test]
async fn resume_publishes_only_canonical_committed_session_visible_after_bind() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Capture pre-resume committed session identity.
    let pre_session_id = h.wiring.committed_session().id.clone();

    // Do NOT register any projection participant — there is no such API now.
    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Missing);

    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume should succeed without any projection participant");

    // 1. The committed session is exactly the canonical one we resumed from.
    let committed = h.wiring.committed_session();
    assert_eq!(committed.id, "resume-target");
    assert_ne!(committed.id, pre_session_id);

    // 2. resume publishes *only* the canonical committed session — a freshly
    //    bound run must observe the same session identity, with no stale or
    //    intermediate projection state.
    let bound = h
        .wiring
        .bind_main_run()
        .await
        .expect("bind should succeed after resume");
    assert_eq!(
        bound.session().id,
        committed.id,
        "bound run must observe the canonical committed session published by resume"
    );
}
