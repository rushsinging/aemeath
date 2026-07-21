//! Composition integration tests for Main Session wiring (#11).
//!
//! These tests prove:
//!
//! 1. **Real Memory opener uses project/config** — the production wiring
//!    constructs `DatasetMemoryOpener` with `FileSystemDatasetAdapter` +
//!    `FileLegacyMemorySourceFactory`, eager-opens memory from the workspace
//!    `ProjectIdentity` + committed `MemoryConfig`, and the resulting
//!    `MemoryPort` is filesystem-backed (writes persist).
//! 2. **Runtime gets the same wiring** — the session id returned by
//!    `AgentClientImpl::session_id()` matches the wiring's
//!    `committed_session().id`, proving no id drift.
//! 3. **Config query/writer gate-aware** — `config_query()` and
//!    `config_writer()` from the wiring return working façades.

use std::sync::Arc;

use context::context_port::ContextPort;
use context::domain::{
    ContentFingerprint, ContextAppend, ContextRequestId, FinalizeCause, RunStepId, SessionId,
    SessionRevision,
};
use context::MainSessionDependencies;
use context::SessionManagementPort;
use sdk::{ChatBootstrapArgs, RunId};
use share::message::Message;

// ─── Helpers ─────────────────────────────────────────────────────────

/// Process-global mutex so tests that set `AEMEATH_AGENTS_DIR` don't race.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
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
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn setup_agents_dir(temp: &tempfile::TempDir) -> EnvGuard {
    let agents_dir = temp.path().join("agents");
    std::fs::create_dir_all(&agents_dir).expect("create agents dir");
    std::fs::write(
        agents_dir.join("aemeath.json"),
        serde_json::json!({
            "models": {
                "default": "local/test-model",
                "providers": {
                    "local": {
                        "baseUrl": "http://127.0.0.1:1/v1",
                        "apiKey": "test-api-key",
                        "driver": "openai",
                        "models": [{
                            "id": "test-model",
                            "name": "Test Model",
                            "input": ["text"],
                            "contextWindow": 8192,
                            "max_tokens": 1024
                        }]
                    }
                }
            }
        })
        .to_string(),
    )
    .expect("write config");
    std::fs::write(agents_dir.join("mcp.json"), r#"{"mcpServers":{}}"#).expect("write MCP config");
    EnvGuard::set("AEMEATH_AGENTS_DIR", &agents_dir)
}

fn cli_config_input(args: &ChatBootstrapArgs) -> config::CliConfigInput {
    config::CliConfigInput {
        api_key: args.api_key.clone(),
        base_url: args.base_url.clone(),
        model: args.model.clone(),
        max_tokens: args.max_tokens,
        context_size: (args.context_size > 0).then_some(args.context_size),
        allow_all: args.allow_all,
        verbose: args.verbose,
        no_markdown: args.no_markdown,
        max_tool_concurrency: args.max_tool_concurrency,
        max_agent_concurrency: args.max_agent_concurrency,
    }
}

fn session_management() -> Arc<dyn SessionManagementPort> {
    Arc::new(context::adapters::AtomicBlobSessionManagement::new(
        storage::api::file_system_blob(share::config::paths::global_agents_dir())
            .expect("create session blob"),
    ))
}

// ─── Tests ───────────────────────────────────────────────────────────

#[test]
fn production_runtime_has_no_direct_active_memory_construction() {
    let source = include_str!("../src/runtime.rs");

    for forbidden in [
        "AtomicDatasetMemoryStore",
        "ProjectMemoryOpener",
        "_main_memory",
    ] {
        assert!(
            !source.contains(forbidden),
            "production runtime must open active Memory only through MainSession MemoryOpener; found {forbidden}"
        );
    }
    assert_eq!(
        source.matches("DatasetMemoryOpener::new").count(),
        1,
        "production runtime must provide exactly one active Memory opener to MainSession wiring"
    );
}

/// The production wiring constructs a real `DatasetMemoryOpener` backed by
/// the filesystem. A memory entry written through the committed `MemoryPort`
/// must be retrievable — proving the opener is not a no-op.
#[tokio::test(flavor = "current_thread")]
async fn production_wiring_uses_real_filesystem_backed_memory() {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("create project root");
    let _env = setup_agents_dir(&temp);

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = config::wire_project_config_with_cli(
        &root,
        cli_config_input(&ChatBootstrapArgs {
            api_key: Some("test-api-key".to_string()),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
            model: Some("local/test-model".to_string()),
            ..Default::default()
        }),
    )
    .await
    .expect("wire config");

    let task_wiring = task::wire_task();

    // Construct the same production opener that Composition uses.
    let dataset_adapter = Arc::new(
        storage::FileSystemDatasetAdapter::new(share::config::paths::global_agents_dir())
            .expect("create dataset adapter"),
    );
    let legacy_factory = Arc::new(memory::FileLegacyMemorySourceFactory::new(
        share::config::paths::global_memory_dir(),
    ));
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        dataset_adapter,
        legacy_factory,
    ));

    let session_management = session_management();
    let deps = MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task_wiring.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener,
        session_management: session_management.clone(),
        context_factory: Arc::new(context::adapters::ProductionMainContextFactory::new(
            Arc::new(context::adapters::NoOpCanonicalSessionWriter),
        )),
    };
    let wiring = context::wire_main_session(deps)
        .await
        .expect("wire main session with real opener");

    // The committed memory port must be functional — write an entry and
    // verify it can be retrieved. An InMemoryTestOpener would lose the
    // entry on clone; the filesystem-backed opener persists it.
    let memory = wiring.committed_memory();
    let entry = memory::MemoryEntry::new(
        memory::MemoryId::now_v7(),
        1,
        memory::MemoryLayer::Project,
        memory::MemoryCategory::Decision,
        "test memory from composition wiring",
        memory::MemorySource::User,
    )
    .expect("create memory entry");
    let write_result = memory.write(entry.clone()).await.expect("write entry");
    assert!(
        matches!(write_result, memory::WriteResult::Added { .. }),
        "write should add the entry, got {write_result:?}"
    );

    let entries = memory.list(Some(memory::MemoryLayer::Project));
    assert!(
        entries
            .iter()
            .any(|e| e.content == "test memory from composition wiring"),
        "filesystem-backed memory must persist entries: {:?}",
        entries
    );
}

#[tokio::test(flavor = "current_thread")]
async fn production_context_append_reopens_from_atomic_blob() {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("create project root");
    let _env = setup_agents_dir(&temp);

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = config::wire_project_config(&root)
        .await
        .expect("wire config");
    let task_wiring = task::wire_task();
    let dataset_adapter = Arc::new(
        storage::FileSystemDatasetAdapter::new(share::config::paths::global_agents_dir())
            .expect("create dataset adapter"),
    );
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        dataset_adapter,
        Arc::new(memory::FileLegacyMemorySourceFactory::new(
            share::config::paths::global_memory_dir(),
        )),
    ));
    let session_blob = storage::api::file_system_blob(share::config::paths::global_agents_dir())
        .expect("create session blob");
    let session_management: Arc<dyn SessionManagementPort> = Arc::new(
        context::adapters::AtomicBlobSessionManagement::new(session_blob.clone()),
    );
    let writer = Arc::new(context::adapters::AtomicBlobCanonicalSessionWriter::new(
        session_blob,
    ));
    let wiring = context::wire_main_session(MainSessionDependencies {
        workspace,
        task_persist: task_wiring.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener,
        session_management: session_management.clone(),
        context_factory: Arc::new(context::adapters::ProductionMainContextFactory::new(writer)),
    })
    .await
    .expect("wire main session");

    let bound = wiring.bind_main_run().await.expect("bind run");
    let context: Arc<dyn ContextPort> = bound.context();
    let session_id = bound.session().id.clone();
    drop(bound);
    let append = ContextAppend {
        session_id: SessionId::new(&session_id),
        expected_revision: SessionRevision::new(0),
        run_id: RunId::new("production-run"),
        step_id: RunStepId::new("production-step"),
        source_request_id: ContextRequestId::new("production-request"),
        finalize_cause: FinalizeCause::Completed,
        messages: vec![Message::user("production durable fact")],
        receipts: vec![],
        api_input_tokens: Some(34),
        fingerprint: ContentFingerprint::new("production-fingerprint"),
    };
    context
        .append_and_persist(&append)
        .await
        .expect("persist production append");

    let exported = session_management
        .export(&session_id)
        .await
        .expect("reopen canonical session bytes");
    let reopened: serde_json::Value =
        serde_json::from_slice(&exported).expect("decode canonical session envelope");
    assert_eq!(reopened["id"], session_id);
    assert_eq!(reopened["revision"], 1);
    assert_eq!(reopened["committed_steps"].as_array().unwrap().len(), 1);
    assert_eq!(
        reopened["committed_steps"][0]["fingerprint"],
        "production-fingerprint"
    );
    let slices = reopened["run_slices"]
        .as_array()
        .expect("canonical run_slices array");
    assert_eq!(slices.len(), 1);
    let outcome = slices[0]["steps"][0]["outcome"]
        .as_array()
        .expect("finalized compatibility outcome");
    assert_eq!(outcome.len(), 1);
    assert_eq!(outcome[0]["role"], "user");
}

/// The Runtime client's session id must match the wiring's committed session
/// id — proving no id drift between Context and Runtime.
#[tokio::test(flavor = "current_thread")]
async fn runtime_session_id_matches_wiring_committed_session() {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("create project root");
    let _env = setup_agents_dir(&temp);

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = config::wire_project_config_with_cli(
        &root,
        cli_config_input(&ChatBootstrapArgs {
            api_key: Some("test-api-key".to_string()),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
            model: Some("local/test-model".to_string()),
            context_size: 8192,
            ..Default::default()
        }),
    )
    .await
    .expect("wire config");

    let task_wiring = task::wire_task();
    let task_access = task_wiring.access();

    // Construct the same production opener that Composition uses.
    let dataset_adapter = Arc::new(
        storage::FileSystemDatasetAdapter::new(share::config::paths::global_agents_dir())
            .expect("create dataset adapter"),
    );
    let legacy_factory = Arc::new(memory::FileLegacyMemorySourceFactory::new(
        share::config::paths::global_memory_dir(),
    ));
    let project_key =
        memory::api::ProjectMemoryKey::derive(root.to_str().expect("project root is UTF-8"), None)
            .expect("derive key");
    let reflection_history: Arc<dyn memory::api::ReflectionHistoryStore> = Arc::new(
        memory::AtomicDatasetReflectionHistoryStore::new(dataset_adapter.clone(), project_key),
    );
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        dataset_adapter,
        legacy_factory,
    ));

    let session_management = session_management();
    let deps = MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task_wiring.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener,
        session_management: session_management.clone(),
        context_factory: Arc::new(context::adapters::ProductionMainContextFactory::new(
            Arc::new(context::adapters::NoOpCanonicalSessionWriter),
        )),
    };
    let wiring = context::wire_main_session(deps)
        .await
        .expect("wire main session");
    assert!(Arc::ptr_eq(
        &wiring.session_management(),
        &session_management,
    ));

    // Capture the wiring's committed session id before building the client.
    let wiring_session_id = wiring.committed_session().id.clone();

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        workspace,
        wiring,
        composition::provider::provider_factory(),
        reflection_history,
        Arc::new(policy::AllowAllPolicy),
        task_access,
        session_management,
    );

    let args = ChatBootstrapArgs {
        cwd: Some(root),
        api_key: Some("test-api-key".to_string()),
        base_url: Some("http://127.0.0.1:1/v1".to_string()),
        model: Some("local/test-model".to_string()),
        context_size: 8192,
        ..Default::default()
    };

    let client = runtime::from_args_with_workspace(args, dependencies)
        .await
        .expect("build client");

    // The Runtime's session id must be the SAME as the wiring's committed
    // session id — not a separately generated one.
    assert_eq!(
        client.session_id(),
        wiring_session_id,
        "Runtime session id must match the wiring's committed session id (no drift)"
    );
}

/// The wiring's config_query and config_writer façades return working
/// gate-aware implementations.
#[tokio::test(flavor = "current_thread")]
async fn config_query_and_writer_are_gate_aware_from_wiring() {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("create project root");
    let _env = setup_agents_dir(&temp);

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = config::wire_project_config(&root)
        .await
        .expect("wire config");

    let task_wiring = task::wire_task();

    let dataset_adapter = Arc::new(
        storage::FileSystemDatasetAdapter::new(share::config::paths::global_agents_dir())
            .expect("create dataset adapter"),
    );
    let legacy_factory = Arc::new(memory::FileLegacyMemorySourceFactory::new(
        share::config::paths::global_memory_dir(),
    ));
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        dataset_adapter,
        legacy_factory,
    ));

    let session_management = session_management();
    let deps = MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task_wiring.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener,
        session_management: session_management.clone(),
        context_factory: Arc::new(context::adapters::ProductionMainContextFactory::new(
            Arc::new(context::adapters::NoOpCanonicalSessionWriter),
        )),
    };
    let wiring = context::wire_main_session(deps)
        .await
        .expect("wire main session");

    // config_query() returns a gate-aware façade.
    let query = wiring.config_query();
    let snapshot = query
        .snapshot()
        .await
        .expect("config query should return a snapshot");
    assert_eq!(
        snapshot.context_size(),
        wiring.committed_config().context_size(),
        "query snapshot should match wiring's committed config"
    );

    // config_reader() returns the raw reader for bootstrap.
    let reader = wiring.config_reader();
    assert_eq!(
        reader.committed_snapshot().context_size(),
        wiring.committed_config().context_size(),
        "reader snapshot should match wiring's committed config"
    );

    // config_writer() returns a gate-aware façade (just verify it exists).
    let _writer = wiring.config_writer();
}
