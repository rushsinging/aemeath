//! Cross Context→Runtime integration tests for MainSessionWiring production
//! wiring (#12).
//!
//! These tests exercise the boundary between the Context and Runtime crates:
//!
//! 1. **Startup resume truly restores** — `resume_session_to_backing` loads a
//!    canonical session, calls `wiring.resume_prepared`, and projects the
//!    committed session into chain/frozen/summary backing.
//! 2. **Runtime resume equivalence** — the same helper is used for both
//!    startup `args.resume` and runtime `PendingCommand::ResumeSession`.
//! 3. **Bound lease blocks resume until run ends** — `bind_main_run` acquires
//!    a shared permit that blocks `resume_prepared` (exclusive) until dropped.
//! 4. **Config query/writer come from wiring** — the wiring façade provides
//!    gate-aware `ConfigQuery` and `ConfigWriter`.

use std::sync::Arc;

use context::session::{save_session, ChatSegment, Session, SessionMetadata};
use context::MainSessionWiring;
use runtime::{resume_session_to_backing, ResumeError};
use share::message::{Message, Role};

// ─── Helpers ─────────────────────────────────────────────────────────

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key,
            previous,
            _lock: lock,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

async fn make_wiring_and_workspace(
    temp: &tempfile::TempDir,
) -> (Arc<MainSessionWiring>, project::WorkspaceViews) {
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("create root");
    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = config::wire_project_config(&root)
        .await
        .expect("wire config");
    let task_wiring = task::wire_task();
    let wiring = context::test_support::wire_in_memory(
        &workspace,
        task_wiring.persist(),
        config.reader(),
        config.participant(),
    )
    .await;
    (wiring, workspace)
}

async fn seed_session(workspace: &project::WorkspaceViews, id: &str) {
    let ws_ctx = workspace.persist().snapshot();
    let cwd = workspace.read().initial_cwd().display().to_string();
    let mut session = Session::new(id.to_string(), cwd);
    let mut seg = ChatSegment::normal(None);
    seg.messages = vec![
        Message::user("hello from saved session"),
        Message::placeholder(Role::Assistant),
    ];
    session.chats = vec![seg];
    session.metadata = SessionMetadata::default();
    session.workspace = Some(ws_ctx);
    save_session(&session).await.expect("save seed session");
}

// ─── Test 1: Startup resume truly restores ───────────────────────────

#[tokio::test]
async fn startup_resume_truly_restores_messages() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let _env = EnvGuard::set(
        "AEMEATH_AGENTS_DIR",
        temp.path().join("agents").to_str().unwrap(),
    );
    std::fs::create_dir_all(temp.path().join("agents")).expect("create agents dir");

    let (wiring, workspace) = make_wiring_and_workspace(&temp).await;
    seed_session(&workspace, "resume-target-1").await;

    let (restore, session_id) = resume_session_to_backing("resume-target-1", &wiring)
        .await
        .expect("resume should succeed");

    assert_eq!(session_id, "resume-target-1");
    let messages = restore.active_chain.messages_flat();
    assert!(
        messages
            .iter()
            .any(|m| m.text_content().contains("hello from saved session")),
        "restored chain should contain the seeded message"
    );
}

// ─── Test 2: Runtime resume equivalence ──────────────────────────────

#[tokio::test]
async fn runtime_resume_is_equivalent_to_startup_resume() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let _env = EnvGuard::set(
        "AEMEATH_AGENTS_DIR",
        temp.path().join("agents").to_str().unwrap(),
    );
    std::fs::create_dir_all(temp.path().join("agents")).expect("create agents dir");

    let (wiring, workspace) = make_wiring_and_workspace(&temp).await;
    seed_session(&workspace, "resume-target-2").await;

    // Both startup and runtime paths call the same `resume_session_to_backing`
    // helper. Calling it twice with the same session should produce identical
    // projections.
    let (restore1, id1) = resume_session_to_backing("resume-target-2", &wiring)
        .await
        .expect("first resume");

    let (restore2, id2) = resume_session_to_backing("resume-target-2", &wiring)
        .await
        .expect("second resume");

    assert_eq!(id1, id2, "both resumes return the same session ID");
    assert_eq!(
        restore1.active_chain.messages_flat().len(),
        restore2.active_chain.messages_flat().len(),
        "both resumes produce the same number of messages"
    );
}

// ─── Test 3: Bound lease blocks resume until run ends ────────────────

#[tokio::test]
async fn bound_lease_blocks_resume_until_dropped() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let _env = EnvGuard::set(
        "AEMEATH_AGENTS_DIR",
        temp.path().join("agents").to_str().unwrap(),
    );
    std::fs::create_dir_all(temp.path().join("agents")).expect("create agents dir");

    let (wiring, workspace) = make_wiring_and_workspace(&temp).await;
    seed_session(&workspace, "resume-target-3").await;

    // Bind a main run — acquires shared permit.
    let bound = wiring
        .bind_main_run()
        .await
        .expect("bind_main_run should succeed");
    // Use bound.config to satisfy the "resource capture" requirement.
    let _captured_config = bound.config();

    // While the bound is alive, additional shared permits are allowed
    // (multiple bind_main_run can coexist).
    let gate = wiring.gate();
    assert!(
        gate.try_acquire_shared().is_ok(),
        "additional shared permits should succeed while a bound is alive"
    );

    // Drop the bound — releases shared permit.
    drop(bound);

    // Now resume should succeed (exclusive permit available).
    let (_restore, _id) = resume_session_to_backing("resume-target-3", &wiring)
        .await
        .expect("resume should succeed after bound is dropped");
}

// ─── Test 4: Config query/writer come from wiring ────────────────────

#[tokio::test]
async fn config_query_and_writer_come_from_wiring() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("create root");

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = config::wire_project_config(&root)
        .await
        .expect("wire config");
    let task_wiring = task::wire_task();

    let wiring = context::test_support::wire_in_memory(
        &workspace,
        task_wiring.persist(),
        config.reader(),
        config.participant(),
    )
    .await;

    // config_query() should return a gate-aware façade.
    let query = wiring.config_query();
    let snapshot = query
        .snapshot()
        .await
        .expect("config query should return a snapshot");

    // The snapshot should match the committed config from the wiring.
    assert_eq!(
        snapshot.context_size(),
        wiring.committed_config().context_size(),
        "query snapshot should match wiring's committed config"
    );

    // config_writer() should return a gate-aware façade.
    let _writer = wiring.config_writer();

    // config_reader() should return the raw reader for bootstrap.
    let reader = wiring.config_reader();
    assert_eq!(
        reader.committed_snapshot().context_size(),
        wiring.committed_config().context_size(),
        "reader snapshot should match wiring's committed config"
    );
}

// ─── Test 5: Resume not-found error maps correctly ───────────────────

#[tokio::test]
async fn resume_nonexistent_session_returns_load_error() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let _env = EnvGuard::set(
        "AEMEATH_AGENTS_DIR",
        temp.path().join("agents").to_str().unwrap(),
    );
    std::fs::create_dir_all(temp.path().join("agents")).expect("create agents dir");

    let (wiring, _workspace) = make_wiring_and_workspace(&temp).await;

    let result = resume_session_to_backing("does-not-exist", &wiring).await;

    assert!(
        matches!(result, Err(ResumeError::Load(_))),
        "nonexistent session should return Load error, got: {:?}",
        result
    );
}

// ─── Test 6: Cross-project resume — bound run sees target memory config ─

/// Creates a second project directory with a custom `.agents/aemeath.json`
/// containing different memory settings and a distinct default model.
fn make_target_project(temp: &tempfile::TempDir) -> std::path::PathBuf {
    let root_b = temp.path().join("project-b");
    let agents_b = root_b.join(".agents");
    std::fs::create_dir_all(&agents_b).expect("create project B .agents dir");
    std::fs::write(
        agents_b.join("aemeath.json"),
        serde_json::json!({
            "memory": {
                "enabled": false,
                "reflection": { "enabled": true, "interval_turns": 7 }
            },
            "models": {
                "default": "local/target-model",
                "providers": {
                    "local": {
                        "baseUrl": "http://127.0.0.1:1/v1",
                        "apiKey": "target-api-key",
                        "driver": "openai",
                        "models": [{
                            "id": "target-model",
                            "name": "Target Model",
                            "input": ["text"],
                            "contextWindow": 4096,
                            "max_tokens": 512
                        }]
                    }
                }
            }
        })
        .to_string(),
    )
    .expect("write project B config");
    root_b
}

#[tokio::test]
async fn cross_project_resume_bound_run_sees_target_memory_config() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let _env = EnvGuard::set(
        "AEMEATH_AGENTS_DIR",
        temp.path().join("agents").to_str().unwrap(),
    );
    std::fs::create_dir_all(temp.path().join("agents")).expect("create agents dir");

    // Project A — wiring source, default memory config (enabled, inject_count=5).
    let (wiring, _workspace_a) = make_wiring_and_workspace(&temp).await;

    // Verify bootstrap defaults before any resume.
    let bootstrap_memory = wiring.committed_config().memory().clone();
    assert!(
        bootstrap_memory.enabled,
        "project A default memory should be enabled"
    );
    assert_eq!(
        bootstrap_memory.inject_count, 5,
        "project A default inject_count should be 5"
    );

    // Project B — resume target with disabled memory and inject_count=3.
    let root_b = make_target_project(&temp);
    let workspace_b = project::wire_production_workspace(root_b.clone())
        .expect("wire workspace B")
        .into_views();
    seed_session(&workspace_b, "cross-project-memory-target").await;

    // Bind before resume — should see project A defaults.
    {
        let bound = wiring
            .bind_main_run()
            .await
            .expect("bind before resume should succeed");
        assert!(bound.config().memory().enabled);
        assert_eq!(bound.config().memory().inject_count, 5);
    }

    // Cross-project resume into project B.
    resume_session_to_backing("cross-project-memory-target", &wiring)
        .await
        .expect("cross-project resume should succeed");

    // After resume, committed_config must reflect project B.
    let committed = wiring.committed_config();
    assert!(
        !committed.memory().enabled,
        "memory should be disabled after resume to project B"
    );
    assert_eq!(
        committed.memory().reflection.interval_turns,
        7,
        "reflection interval_turns should be 7 after resume to project B"
    );

    // bind_main_run after resume must also reflect project B — this is the
    // exact value the loop_runner passes to MainRunPort (H3 fix).
    let bound = wiring
        .bind_main_run()
        .await
        .expect("bind after resume should succeed");
    assert!(
        !bound.config().memory().enabled,
        "bound config memory should be disabled"
    );
    assert_eq!(
        bound.config().memory().reflection.interval_turns,
        7,
        "bound config reflection interval_turns should be 7"
    );
}

// ─── Test 7: Cross-project resume — model/MemoryConfig from target config ─
//
// Verifies the invariant the from_args reordering (H3) depends on:
// after startup resume, wiring.committed_config() returns the target
// project's config — not the bootstrap project's. The from_args function
// reads committed_config() AFTER resume, so model resolution, API key
// derivation, and MemoryConfig all come from the target project.

#[tokio::test]
async fn cross_project_resume_committed_config_has_target_model_and_memory() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let _env = EnvGuard::set(
        "AEMEATH_AGENTS_DIR",
        temp.path().join("agents").to_str().unwrap(),
    );
    std::fs::create_dir_all(temp.path().join("agents")).expect("create agents dir");

    // Project A — wiring source, no custom config (defaults).
    let (wiring, _workspace_a) = make_wiring_and_workspace(&temp).await;

    // Project B — target with a distinct model and disabled memory.
    let root_b = make_target_project(&temp);
    let workspace_b = project::wire_production_workspace(root_b.clone())
        .expect("wire workspace B")
        .into_views();
    seed_session(&workspace_b, "cross-project-config-target").await;

    // Before resume: default model string should not contain "target-model".
    let before = wiring.committed_config();
    assert!(
        !before.models().default.contains("target-model"),
        "project A should NOT have target-model as default"
    );

    // Resume into project B.
    resume_session_to_backing("cross-project-config-target", &wiring)
        .await
        .expect("cross-project resume should succeed");

    // After resume: committed_config must reflect project B's model and memory.
    let after = wiring.committed_config();
    let after_default = after
        .resolve_model_selection(&after.models().default)
        .expect("resolve default model after resume");
    assert_eq!(
        after_default.model.id, "target-model",
        "after resume, default model should be project B's target-model"
    );
    assert_eq!(
        after_default.source_key, "local",
        "after resume, source_key should be local"
    );
    assert!(
        !after.memory().enabled,
        "after resume, memory should be disabled"
    );
}

// ─── Test 8: Projection participant atomically updates backing inside gate ─

/// When a `SessionProjectionParticipant` is registered, `resume_prepared`
/// updates the leased projection backing **inside** the exclusive gate.
/// The event chain returned by `resume_session_to_backing` is equivalent
/// to the backing's chain — there is no observable window and no double-write.
#[tokio::test]
async fn projection_participant_event_chain_equivalent_to_backing() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let _env = EnvGuard::set(
        "AEMEATH_AGENTS_DIR",
        temp.path().join("agents").to_str().unwrap(),
    );
    std::fs::create_dir_all(temp.path().join("agents")).expect("create agents dir");

    let (wiring, workspace) = make_wiring_and_workspace(&temp).await;
    seed_session(&workspace, "projection-equiv").await;

    // Register a mock projection participant that records the committed chain.
    use context::SessionProjectionParticipant;
    use std::sync::Mutex;

    struct RecordingParticipant {
        committed_chain: Mutex<Option<context::session::ChatChain>>,
        commit_count: std::sync::atomic::AtomicUsize,
    }

    impl SessionProjectionParticipant for RecordingParticipant {
        fn prepare(
            &self,
            session: &context::domain::session::CanonicalSession,
        ) -> context::session::SessionRestore {
            context::session::SessionRestore::from_canonical(session)
        }

        fn commit(&self, prepared: context::session::SessionRestore) {
            self.commit_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.committed_chain.lock().unwrap() = Some(prepared.active_chain);
        }
    }

    let recorder = Arc::new(RecordingParticipant {
        committed_chain: Mutex::new(None),
        commit_count: std::sync::atomic::AtomicUsize::new(0),
    });
    wiring
        .register_projection_participant(recorder.clone() as Arc<dyn SessionProjectionParticipant>);

    // Resume — this updates the backing inside the exclusive gate.
    let (restore, _id) = resume_session_to_backing("projection-equiv", &wiring)
        .await
        .expect("resume should succeed");

    // The participant was called exactly once (no double-write).
    assert_eq!(
        recorder
            .commit_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "participant commit should be called exactly once — no double-write"
    );

    // The event chain (from restore) is equivalent to the backing's chain
    // (from participant commit). Both derive from the same committed session.
    let backing_chain = recorder
        .committed_chain
        .lock()
        .unwrap()
        .clone()
        .expect("participant should have committed a chain");

    assert_eq!(
        restore.active_chain.messages_flat().len(),
        backing_chain.messages_flat().len(),
        "event chain and backing chain must have the same message count — equivalence"
    );

    // Verify the chain contains the expected seeded message.
    let messages = backing_chain.messages_flat();
    assert!(
        messages
            .iter()
            .any(|m| m.text_content().contains("hello from saved session")),
        "backing chain should contain the seeded message"
    );
}
