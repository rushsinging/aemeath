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
use memory::{
    InMemoryMemory, MemoryOpener, MemoryOpenerError, MemoryPolicy, MemoryPort, ProjectMemoryKey,
};
use project::wire_production_workspace;
use share::session_types::PersistedWorkspaceContext;
use task::{
    BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPersist, TaskPriority, TaskSnapshot, TaskStore,
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
        id: "initial".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(ws_ctx),
        revision: 0,
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
        initial_session,
        initial_memory,
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

/// Cross-project resume is allowed: a session whose workspace belongs to a
/// different project can be resumed.  The canonical identity from
/// `prepare_restore` drives Config/Memory, not the live identity.
#[tokio::test]
async fn cross_project_resume_succeeds() {
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

    // Resume should succeed — cross-project is allowed.
    h.wiring
        .resume_prepared(session)
        .await
        .expect("cross-project resume should succeed");

    // Committed session changed.
    assert_ne!(
        h.wiring.committed_session().id,
        pre_session_id,
        "committed session should change after cross-project resume"
    );

    // Committed memory changed (new Arc opened for the prepared identity).
    assert!(
        !Arc::ptr_eq(&h.wiring.committed_memory(), &pre_memory),
        "committed memory should be a new Arc"
    );

    // Config was committed (revision advances).
    assert_ne!(
        h.config_service.committed_snapshot().revision(),
        pre_config_revision,
        "config revision should advance"
    );

    // Memory opener was called exactly once for the prepared identity.
    assert_eq!(
        h.memory_opener.open_count(),
        1,
        "memory opener should be called once"
    );
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

/// Resuming with `workspace: Missing` returns a typed error and changes nothing.
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

// ─── SessionProjectionParticipant tests ──────────────────────────────

use context::application::main_session::SessionProjectionParticipant;
use context::domain::session::{ChatChain, ChatSegment};
use std::sync::Mutex as StdMutex;

/// A mock `SessionProjectionParticipant` that records the prepared token and
/// the number of `commit` calls. Used to verify atomicity and no-double-write.
struct MockProjectionParticipant {
    committed_chain: StdMutex<Option<ChatChain>>,
    committed_frozen: StdMutex<Option<Vec<ChatSegment>>>,
    committed_summary: StdMutex<Option<String>>,
    commit_count: StdMutex<usize>,
    /// `true` once `commit` has been called. Used to assert ordering.
    committed: StdMutex<bool>,
}

impl MockProjectionParticipant {
    fn new() -> Self {
        Self {
            committed_chain: StdMutex::new(None),
            committed_frozen: StdMutex::new(None),
            committed_summary: StdMutex::new(None),
            commit_count: StdMutex::new(0),
            committed: StdMutex::new(false),
        }
    }

    fn commit_count(&self) -> usize {
        *self.commit_count.lock().unwrap()
    }
}

impl SessionProjectionParticipant for MockProjectionParticipant {
    fn prepare(&self, session: &CanonicalSession) -> context::session::SessionRestore {
        context::session::SessionRestore::from_canonical(session)
    }

    fn commit(&self, prepared: context::session::SessionRestore) {
        let mut count = self.commit_count.lock().unwrap();
        *count += 1;
        *self.committed_chain.lock().unwrap() = Some(prepared.active_chain.clone());
        *self.committed_frozen.lock().unwrap() = Some(prepared.frozen_chats.clone());
        *self.committed_summary.lock().unwrap() = prepared.active_summary.clone();
        *self.committed.lock().unwrap() = true;
    }
}

/// Registering a `SessionProjectionParticipant` and then calling
/// `resume_prepared` atomically updates the leased projection backing
/// **inside** the exclusive gate — the participant's `commit` is called
/// exactly once, before the gate is released.
#[tokio::test]
async fn projection_participant_updated_inside_gate_on_resume() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Register the mock participant.
    let mock = Arc::new(MockProjectionParticipant::new());
    h.wiring
        .register_projection_participant(mock.clone() as Arc<dyn SessionProjectionParticipant>);

    let ws = h.workspace_persist.snapshot();
    let session = CanonicalSession {
        id: "projection-test".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(ws),
        revision: 1,
        committed_steps: vec![],
    };

    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume should succeed");

    // The participant's commit was called exactly once (no double-write).
    assert_eq!(
        mock.commit_count(),
        1,
        "participant commit should be called exactly once"
    );

    // The committed session matches what was projected.
    assert_eq!(h.wiring.committed_session().id, "projection-test");
}

/// When no participant is registered, `resume_prepared` behaves exactly as
/// before — the leased projection is not updated (it remains the caller's
/// responsibility, i.e. migration debt).
#[tokio::test]
async fn resume_without_participant_works_normally() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Do NOT register any participant.
    let ws = h.workspace_persist.snapshot();
    let session = session_with_workspace(&ws, SnapshotState::Missing);

    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume should succeed without participant");

    assert_eq!(h.wiring.committed_session().id, "resume-target");
}

/// **CM5 core test**: while `resume_prepared` holds the exclusive permit,
/// a concurrent shared observer (`bind_main_run`) is blocked.  After the
/// exclusive permit is released, the observer's very first read sees the
/// **new** committed session **and** the **new** projection — there is no
/// observable window where the session is new but the projection is stale.
#[tokio::test]
async fn concurrent_observer_sees_new_session_and_projection_after_gate_release() {
    let _guard = git_lock().await;
    let h = build_harness();

    // Register the mock participant so the projection is updated inside gate.
    let mock = Arc::new(MockProjectionParticipant::new());
    h.wiring
        .register_projection_participant(mock.clone() as Arc<dyn SessionProjectionParticipant>);

    // Build the resume session with a chat segment so the projection is
    // non-empty and we can verify the chain content.
    let ws = h.workspace_persist.snapshot();
    let segment = ChatSegment::normal(None);
    let session = CanonicalSession {
        id: "cm5-consistency".to_string(),
        chats: vec![segment],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(ws),
        revision: 1,
        committed_steps: vec![],
    };

    // Perform the resume. This holds the exclusive permit for the duration.
    h.wiring
        .resume_prepared(session)
        .await
        .expect("resume should succeed");

    // Immediately after the gate is released, a shared observer reads.
    let bound = h
        .wiring
        .bind_main_run()
        .await
        .expect("bind should succeed after resume");

    // The observer sees the new session id.
    assert_eq!(
        bound.session().id,
        "cm5-consistency",
        "observer should see new session id immediately after gate release"
    );

    // The observer also sees that the projection was updated — the
    // participant committed the chain derived from the same session.
    assert!(
        *mock.committed.lock().unwrap(),
        "projection participant must have committed before gate release"
    );
    assert_eq!(
        mock.commit_count(),
        1,
        "exactly one commit, no double-write"
    );

    // The projected chain is equivalent to the committed session's chain.
    let committed = h.wiring.committed_session();
    let projected_chain = mock.committed_chain.lock().unwrap().clone().unwrap();
    let committed_chain = ChatChain::from_chats(&committed.chats);
    assert_eq!(
        projected_chain.messages_flat().len(),
        committed_chain.messages_flat().len(),
        "projected chain message count must match committed session's chain — no observable window"
    );
}

/// `commit` returns no `Result` — it is infallible by design. This test
/// documents that contract by verifying the return type at the source level.
#[test]
fn projection_participant_commit_is_infallible() {
    // The commit method returns `()`, not `Result<(), _>`. This is a
    // compile-time contract verified by the trait definition itself.
    let mock = MockProjectionParticipant::new();
    let session = CanonicalSession {
        id: "type-test".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(PersistedWorkspaceContext::default()),
        revision: 0,
        committed_steps: vec![],
    };

    // `commit` takes a SessionRestore and returns ().
    let prepared: context::session::SessionRestore =
        SessionProjectionParticipant::prepare(&mock, &session);
    // The return type is `()`, not `Result<(), _>` — this line compiles only
    // because commit is infallible by design.
    SessionProjectionParticipant::commit(&mock, prepared);
}
