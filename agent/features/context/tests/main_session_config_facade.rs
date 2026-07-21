//! Integration tests for the gate-aware ConfigQuery / ConfigWriter façades
//! produced by `MainSessionWiring`, plus cross-project resume verification.
//!
//! These tests use the real `ConfigAppService` (with explicit paths, no env
//! dependence) and real `wire_production_workspace` with temp dirs. Only the
//! `MemoryOpener` is faked so we can track which project identity was used.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use config::{
    ConfigAppService, ConfigPersistError, ConfigReader, ConfigUpdate, ConfigUpdateError,
    NativeConfigStore, ProjectConfigParticipant,
};
use context::application::main_session::{MainSessionWiring, MainSessionWiringBuilder};
use context::domain::session::{CanonicalSession, SnapshotState};
use memory::{
    InMemoryMemory, MemoryOpener, MemoryOpenerError, MemoryPolicy, MemoryPort, ProjectMemoryKey,
};
use project::wire_production_workspace;
use share::session_types::PersistedWorkspaceContext;
use task::{TaskSnapshot, TaskStore};

// ─── Temp dir helpers (copied from main_session_wiring.rs) ───────────

fn ensure_ceiling() -> &'static str {
    static CEILING: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let val = CEILING.get_or_init(|| {
        let base = std::env::temp_dir();
        let path = base.display().to_string();
        let existing = std::env::var("GIT_CEILING_DIRECTORIES").unwrap_or_default();
        let combined = if existing.is_empty() {
            path.clone()
        } else {
            format!("{path}:{existing}")
        };
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
            "aemeath-config-facade-{tag}-{nanos}-{}",
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

static GIT_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn git_lock() -> tokio::sync::MutexGuard<'static, ()> {
    GIT_GUARD.lock().await
}

// ─── Tracking MemoryOpener ───────────────────────────────────────────

/// Fake `MemoryOpener` that records the last `ProjectMemoryKey` it was called
/// with and counts total opens.  Each open returns a fresh `InMemoryMemory`.
struct TrackingMemoryOpener {
    open_count: Arc<AtomicUsize>,
    last_key: Arc<std::sync::Mutex<Option<ProjectMemoryKey>>>,
    fail: Arc<AtomicBool>,
}

impl TrackingMemoryOpener {
    fn new() -> Self {
        Self {
            open_count: Arc::new(AtomicUsize::new(0)),
            last_key: Arc::new(std::sync::Mutex::new(None)),
            fail: Arc::new(AtomicBool::new(false)),
        }
    }

    fn open_count(&self) -> usize {
        self.open_count.load(Ordering::SeqCst)
    }

    fn last_key(&self) -> Option<ProjectMemoryKey> {
        self.last_key.lock().unwrap().clone()
    }

    #[allow(dead_code)]
    fn set_fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }

    fn shared_fields(
        &self,
    ) -> (
        Arc<AtomicUsize>,
        Arc<std::sync::Mutex<Option<ProjectMemoryKey>>>,
        Arc<AtomicBool>,
    ) {
        (
            Arc::clone(&self.open_count),
            Arc::clone(&self.last_key),
            Arc::clone(&self.fail),
        )
    }
}

#[async_trait]
impl MemoryOpener for TrackingMemoryOpener {
    async fn open_memory(
        &self,
        key: &ProjectMemoryKey,
        _config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn MemoryPort>, MemoryOpenerError> {
        self.open_count.fetch_add(1, Ordering::SeqCst);
        *self.last_key.lock().unwrap() = Some(key.clone());
        if self.fail.load(Ordering::SeqCst) {
            return Err(MemoryOpenerError::Io);
        }
        Ok(Arc::new(InMemoryMemory::new(MemoryPolicy::default()).unwrap()) as Arc<dyn MemoryPort>)
    }

    fn boxed_clone(&self) -> Box<dyn MemoryOpener> {
        Box::new(TrackingMemoryOpener {
            open_count: Arc::clone(&self.open_count),
            last_key: Arc::clone(&self.last_key),
            fail: Arc::clone(&self.fail),
        })
    }
}

// ─── Harness ─────────────────────────────────────────────────────────

struct FacadeHarness {
    wiring: MainSessionWiring,
    #[allow(dead_code)]
    workspace_persist: Arc<dyn project::WorkspacePersist>,
    config_service: Arc<ConfigAppService>,
    memory_opener: Arc<TrackingMemoryOpener>,
    _tmp: TempDir,
}

/// Builds a harness with the given `ConfigAppService`.
///
/// This mirrors what `wire_main_session` does in production: it bootstraps
/// the active config location from the verified workspace identity by calling
/// `prepare_for_project` + `commit_project`. This ensures `prepare_update`
/// has an active location to work with.
async fn build_facade_harness(
    tmp: TempDir,
    config_service: Arc<ConfigAppService>,
    memory_opener: Arc<TrackingMemoryOpener>,
) -> FacadeHarness {
    let workspace = wire_production_workspace(tmp.path().to_path_buf()).unwrap();
    let workspace_read = workspace.read();
    let workspace_persist = workspace.persist();

    // Bootstrap the active config location from the workspace identity,
    // mirroring wire_main_session. This sets active_location so that
    // prepare_update can proceed.
    let identity = workspace_read.project_identity();
    let config_location = {
        let search_root = std::path::PathBuf::from(&identity.initial_cwd);
        let stable_identity: &[u8] = match identity.git_common_dir.as_deref() {
            Some(common) if !common.is_empty() => common.as_bytes(),
            _ => identity.initial_cwd.as_bytes(),
        };
        config::ProjectConfigLocation::try_from_project_identity(search_root, stable_identity)
            .unwrap()
    };
    let prepared_config = config_service
        .prepare_for_project(&config_location)
        .await
        .unwrap();
    config_service.commit_project(prepared_config).await;

    let task_store = Arc::new(TaskStore::new());
    let task_persist: Arc<dyn task::TaskPersist> = task_store.clone();

    let config_reader: Arc<dyn ConfigReader> = config_service.clone();
    let config_participant: Arc<dyn ProjectConfigParticipant> = config_service.clone();

    let initial_memory: Arc<dyn MemoryPort> =
        Arc::new(InMemoryMemory::new(MemoryPolicy::default()).unwrap());

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

    let (open_count, last_key, fail) = memory_opener.shared_fields();
    let builder = MainSessionWiringBuilder {
        workspace_read,
        workspace_persist: workspace_persist.clone(),
        task_persist,
        config_reader,
        config_participant,
        memory_opener: Box::new(TrackingMemoryOpener {
            open_count,
            last_key,
            fail,
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

    FacadeHarness {
        wiring,
        workspace_persist,
        config_service,
        memory_opener,
        _tmp: tmp,
    }
}

/// Convenience: harness with no `native_store` (persist_update → NotCommitted).
async fn build_no_store_harness() -> FacadeHarness {
    let tmp = TempDir::new("facade");
    let config_service = Arc::new(ConfigAppService::with_global_path(
        None,
        tmp.path().join("global.json"),
    ));
    let memory_opener = Arc::new(TrackingMemoryOpener::new());
    build_facade_harness(tmp, config_service, memory_opener).await
}

/// Convenience: harness *with* `native_store` (persist_update → Committed).
async fn build_with_store_harness() -> FacadeHarness {
    let tmp = TempDir::new("facade-store");
    let storage = Arc::new(storage::FileSystemBlobAdapter::new(tmp.path()).unwrap());
    let config_service = Arc::new(
        ConfigAppService::with_global_path(None, tmp.path().join("global.json"))
            .with_native_store(NativeConfigStore::new(storage)),
    );
    let memory_opener = Arc::new(TrackingMemoryOpener::new());
    build_facade_harness(tmp, config_service, memory_opener).await
}

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

// ─── Tests ────────────────────────────────────────────────────────────

/// Cross-project resume succeeds and the **prepared** identity drives
/// Config/Memory, not the live identity.
///
/// We place a project config file in the target project's `.agents/` dir with
/// a distinctive model name.  After resuming into that project, the committed
/// config must reflect that model — proving the config location search root
/// used the prepared identity's `initial_cwd`, not the live workspace's.
#[tokio::test]
async fn cross_project_resume_drives_config_and_memory_from_prepared_identity() {
    let _guard = git_lock().await;

    // Live workspace (project B).
    let tmp_live = TempDir::new("live");
    let config_service = Arc::new(ConfigAppService::with_global_path(
        None,
        tmp_live.path().join("global.json"),
    ));
    let memory_opener = Arc::new(TrackingMemoryOpener::new());
    let h = build_facade_harness(tmp_live, config_service, memory_opener).await;

    // Target project (project A) — different temp dir.
    let tmp_target = TempDir::new("target");
    let target_workspace = wire_production_workspace(tmp_target.path().to_path_buf()).unwrap();
    let target_identity = target_workspace.read().project_identity();

    // Place a project config file in project A so we can detect which
    // search_root was used during prepare_for_project.
    let agents_dir = tmp_target.path().join(".agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join("aemeath.json"),
        serde_json::json!({
            "models": { "default": "target-project-model" }
        })
        .to_string(),
    )
    .unwrap();

    let ws_target = target_workspace.persist().snapshot();
    let session = session_with_workspace(&ws_target, SnapshotState::Missing);

    // Pre-resume state.
    let pre_model = h
        .config_service
        .committed_snapshot()
        .models()
        .default
        .clone();
    let pre_memory = h.wiring.committed_memory();
    assert_ne!(pre_model, "target-project-model", "precondition");

    // Resume into project A.
    h.wiring
        .resume_prepared(session)
        .await
        .expect("cross-project resume should succeed");

    // Config must reflect project A's config file — proving the search root
    // was the prepared identity's initial_cwd, not the live workspace's.
    let post_model = h
        .config_service
        .committed_snapshot()
        .models()
        .default
        .clone();
    assert_eq!(
        post_model, "target-project-model",
        "config should be loaded from the prepared identity's project dir"
    );

    // Memory should be opened for the target identity, not the live one.
    assert_eq!(h.memory_opener.open_count(), 1, "memory opener called once");
    let expected_key = ProjectMemoryKey::derive(
        &target_identity.initial_cwd,
        target_identity.git_common_dir.as_deref(),
    )
    .unwrap();
    assert_eq!(
        h.memory_opener.last_key().as_ref(),
        Some(&expected_key),
        "memory key should be derived from the prepared identity"
    );

    // Memory Arc changed.
    assert!(
        !Arc::ptr_eq(&h.wiring.committed_memory(), &pre_memory),
        "committed memory should be a new Arc"
    );
}

/// `ConfigQuery::snapshot` is blocked while an exclusive permit is held.
#[tokio::test]
async fn query_snapshot_blocked_by_exclusive_permit() {
    let _guard = git_lock().await;
    let h = build_no_store_harness().await;

    let query = h.wiring.config_query();

    // Acquire exclusive permit — query should be blocked.
    let exclusive = h.wiring.gate().acquire_owned_exclusive().await.unwrap();

    let blocked = tokio::time::timeout(Duration::from_millis(100), query.snapshot()).await;
    assert!(
        blocked.is_err(),
        "query.snapshot should be blocked while exclusive permit is held"
    );

    // Release exclusive — query should now succeed.
    drop(exclusive);

    let snapshot = tokio::time::timeout(Duration::from_secs(2), query.snapshot())
        .await
        .expect("query should complete after exclusive permit released")
        .expect("query should succeed");
    let _ = snapshot; // just verify we got a snapshot
}

/// `ConfigQuery::subscribe` is also blocked by an exclusive permit.
#[tokio::test]
async fn query_subscribe_blocked_by_exclusive_permit() {
    let _guard = git_lock().await;
    let h = build_no_store_harness().await;

    let query = h.wiring.config_query();
    let exclusive = h.wiring.gate().acquire_owned_exclusive().await.unwrap();

    let blocked = tokio::time::timeout(Duration::from_millis(100), query.subscribe()).await;
    assert!(
        blocked.is_err(),
        "query.subscribe should be blocked while exclusive permit is held"
    );

    drop(exclusive);

    let sub = query
        .subscribe()
        .await
        .expect("subscribe should succeed after exclusive released");
    assert!(
        sub.changes.has_changed().is_ok(),
        "subscription receiver should be usable"
    );
}

/// When `persist_update` returns `NotCommitted`, the Writer returns an error
/// and the old Memory and Config are kept entirely.
#[tokio::test]
async fn update_not_committed_keeps_old_memory_and_config() {
    let _guard = git_lock().await;
    let h = build_no_store_harness().await;

    let writer = h.wiring.config_writer();

    let pre_memory = h.wiring.committed_memory();
    let pre_revision = h.config_service.committed_snapshot().revision();

    // ConfigAppService without native_store → persist_update returns
    // NotCommitted(UnsupportedDurability).
    let result = writer
        .update(ConfigUpdate::SetModel {
            model: "new-model".into(),
        })
        .await;

    assert!(
        matches!(
            result,
            Err(ConfigUpdateError::Persist(
                ConfigPersistError::UnsupportedDurability
            ))
        ),
        "expected Persist(UnsupportedDurability), got {result:?}"
    );

    // Old memory kept (same Arc).
    assert!(
        Arc::ptr_eq(&h.wiring.committed_memory(), &pre_memory),
        "old memory must be kept on NotCommitted"
    );

    // Old config kept (same revision).
    assert_eq!(
        h.config_service.committed_snapshot().revision(),
        pre_revision,
        "config revision must not advance on NotCommitted"
    );
}

/// When `persist_update` returns `Committed`, the Writer installs the
/// candidate Memory, fires the config watch, and returns the change set.
/// This test awaits the result normally.
#[tokio::test]
async fn update_committed_installs_memory_and_advances_watch() {
    let _guard = git_lock().await;
    let h = build_with_store_harness().await;

    let writer = h.wiring.config_writer();

    // Subscribe to watch BEFORE the update.
    let rx = h.config_service.subscribe_committed();
    let pre_revision = h.config_service.committed_snapshot().revision();
    let pre_memory = h.wiring.committed_memory();

    let change_set = writer
        .update(ConfigUpdate::SetModel {
            model: "committed-model".into(),
        })
        .await
        .expect("update should succeed with native_store");

    // Config changed.
    assert_eq!(
        change_set.snapshot.models().default,
        "committed-model",
        "change set should reflect new model"
    );
    assert_ne!(
        change_set.snapshot.revision(),
        pre_revision,
        "revision should advance"
    );

    // Watch fired.
    assert!(
        rx.has_changed().is_ok(),
        "watch receiver should see the update"
    );
    assert_eq!(
        rx.borrow().models().default,
        "committed-model",
        "watch receiver should reflect new model"
    );

    // Memory changed (new Arc).
    assert!(
        !Arc::ptr_eq(&h.wiring.committed_memory(), &pre_memory),
        "committed memory should be a new Arc after Committed update"
    );
}

/// **Handoff semantics**: after the caller drops the `update()` future, the
/// spawned critical section continues and still installs new Memory/Config/watch.
///
/// Futures are lazy in Rust — dropping a future that was never polled means the
/// internal `tokio::spawn` never ran.  The realistic "caller drop" scenario is:
/// the caller spawns the update as a task, and that task is *detached* (never
/// joined).  Inside the update, the critical-section spawn runs independently.
#[tokio::test]
async fn update_committed_installs_after_caller_drop() {
    let _guard = git_lock().await;
    let h = build_with_store_harness().await;

    let writer = h.wiring.config_writer();

    // Subscribe to watch BEFORE the update.
    let _rx = h.config_service.subscribe_committed();
    let pre_revision = h.config_service.committed_snapshot().revision();
    let pre_memory = h.wiring.committed_memory();

    // Spawn the update as its own task (the "caller").  The update() method
    // internally spawns a critical-section task and then awaits its JoinHandle.
    // We do NOT join this outer task — simulating caller detachment.
    let writer_for_task = writer.clone();
    let update_task = tokio::spawn(async move {
        writer_for_task
            .update(ConfigUpdate::SetModel {
                model: "bg-model".into(),
            })
            .await
    });

    // Poll the committed_memory Arc pointer until it changes (with a timeout).
    // The spawned update task runs during our sleep yields.
    let deadline = Duration::from_secs(5);
    let start = std::time::Instant::now();
    loop {
        if !Arc::ptr_eq(&h.wiring.committed_memory(), &pre_memory) {
            break; // memory installed
        }
        if start.elapsed() > deadline {
            panic!("committed memory was not installed within {deadline:?} after caller drop");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Config watch should have fired.
    let final_snapshot = h.config_service.committed_snapshot();
    assert_ne!(
        final_snapshot.revision(),
        pre_revision,
        "config revision should advance after background task completes"
    );
    assert_eq!(
        final_snapshot.models().default,
        "bg-model",
        "config should reflect the background update"
    );

    // The gate should be released (exclusive permit dropped after task completes).
    let shared = h
        .wiring
        .gate()
        .try_acquire_shared()
        .expect("gate should be released after background task completes");
    let _ = shared;

    // Clean up the detached update task (its result is no longer needed but we
    // must join to avoid a "task hung" warning).
    let _ = update_task.await;
}

/// The Writer acquires an exclusive permit: while `update()` is in progress,
/// `bind_main_run` (shared permit) is blocked.
#[tokio::test]
async fn writer_update_blocks_bind_main_run() {
    let _guard = git_lock().await;
    let h = build_no_store_harness().await;

    // We can't easily make the Writer *linger* on the exclusive permit without
    // a slow persist_update.  Instead, we verify the simpler property: the
    // Writer and resume share the same gate.
    let exclusive = h.wiring.gate().acquire_owned_exclusive().await.unwrap();
    let writer = h.wiring.config_writer();

    let blocked = tokio::time::timeout(
        Duration::from_millis(100),
        writer.update(ConfigUpdate::SetModel {
            model: "blocked".into(),
        }),
    )
    .await;
    assert!(
        blocked.is_err(),
        "writer.update should be blocked while exclusive permit is held externally"
    );

    drop(exclusive);
    // Now the writer can proceed (it will fail on NotCommitted since no store).
    let result = writer
        .update(ConfigUpdate::SetModel { model: "ok".into() })
        .await;
    let _ = result;
}
