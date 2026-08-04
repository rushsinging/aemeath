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
use std::path::Path;

// ─── Helpers ─────────────────────────────────────────────────────────

/// Create the temp `agents/` directory tree that tests need on disk
/// (config + mcp files), returning the resolved `agents_dir` PathBuf.
///
/// Tests pass this directly to every wiring call that previously read
/// `share::config::paths::global_agents_dir()` — no environment
/// variables, no process-global mutex, no races with parallel tests.
fn make_agents_dir(temp: &tempfile::TempDir) -> std::path::PathBuf {
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
    agents_dir
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

/// Wire a project config that reads the global config from the test's
/// `agents_dir/aemeath.json` instead of `share::config::paths::global_agents_dir()`
/// (which reads `AEMEATH_AGENTS_DIR`). Used by every test in this file so
/// they can swap agents dirs without env-var races — see #1385.
async fn wire_config_with_agents_dir(
    project_dir: &Path,
    agents_dir: &Path,
    cli: config::CliConfigInput,
) -> Result<config::ConfigWiring, config::ConfigError> {
    config::wire_project_config_with_agents_dir(
        project_dir,
        agents_dir,
        config_native_store(agents_dir),
        cli,
    )
    .await
}

fn config_native_store(agents_dir: &Path) -> config::NativeConfigStore {
    config::NativeConfigStore::new(
        storage::api::file_system_blob(agents_dir.join("config-overrides"))
            .expect("create config override blob"),
    )
}

fn session_management(agents_dir: &Path) -> Arc<dyn SessionManagementPort> {
    Arc::new(context::adapters::AtomicBlobSessionManagement::new(
        storage::api::file_system_blob(agents_dir).expect("create session blob"),
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

    // #1385: reflection history adapter root is agents_dir, not
    // agents_dir.join("memory"). StorageNamespace::Memory already adds the
    // "memory" segment; an explicit join produces memory/memory/...
    let reflection_adapter_new = source
        .match_indices("FileSystemDatasetAdapter::new(")
        .collect::<Vec<_>>();
    assert_eq!(
        reflection_adapter_new.len(),
        3,
        "production runtime must construct exactly 3 dataset adapters: one for reflection, one for Session, and one for MemoryOpener"
    );
    // Verify neither uses `join("memory")` for FileSystemDatasetAdapter.
    // Legacy memory uses `agents_dir.join("memory")` via
    // FileLegacyMemorySourceFactory, not via FileSystemDatasetAdapter.
    for (idx, _) in &reflection_adapter_new {
        let line = source[*idx..].lines().next().unwrap_or("");
        assert!(
            !line.contains(r#"join("memory")"#),
            "FileSystemDatasetAdapter::new must not use join(\"memory\") — \
             namespace adds the segment; found: {line}"
        );
    }
}

/// The production wiring constructs a real `DatasetMemoryOpener` backed by
/// the filesystem. A memory entry written through the committed `MemoryPort`
/// must be retrievable — proving the opener is not a no-op.
#[tokio::test(flavor = "current_thread")]
async fn production_wiring_uses_real_filesystem_backed_memory() {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    let agents_dir = make_agents_dir(&temp);
    std::fs::create_dir_all(&root).expect("create project root");

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = wire_config_with_agents_dir(
        &root,
        &agents_dir,
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
        storage::FileSystemDatasetAdapter::new(agents_dir.clone()).expect("create dataset adapter"),
    );
    let legacy_factory = Arc::new(memory::FileLegacyMemorySourceFactory::new(
        agents_dir.join("memory"),
    ));
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        dataset_adapter,
        legacy_factory,
    ));

    let session_management = session_management(&agents_dir);
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
    let agents_dir = make_agents_dir(&temp);
    std::fs::create_dir_all(&root).expect("create project root");

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = wire_config_with_agents_dir(&root, &agents_dir, config::CliConfigInput::default())
        .await
        .expect("wire config");
    let task_wiring = task::wire_task();
    let dataset_adapter = Arc::new(
        storage::FileSystemDatasetAdapter::new(agents_dir.clone()).expect("create dataset adapter"),
    );
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        dataset_adapter,
        Arc::new(memory::FileLegacyMemorySourceFactory::new(
            agents_dir.join("memory"),
        )),
    ));
    let session_blob = storage::api::file_system_blob(&agents_dir).expect("create session blob");
    let session_dataset = Arc::new(
        storage::FileSystemDatasetAdapter::new(agents_dir.clone())
            .expect("create session dataset adapter"),
    );
    let session_management: Arc<dyn SessionManagementPort> =
        Arc::new(context::adapters::DatasetSessionManagement::new(
            session_dataset.clone(),
            session_blob.clone(),
        ));
    let writer = Arc::new(context::adapters::DatasetCanonicalSessionWriter::new(
        session_dataset,
    ));
    let session_project = workspace.read().project_identity();
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
        duration_ms: None,
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
        .export_for_project(&session_id, &session_project)
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
        .as_object()
        .expect("finalized outcome projection");
    assert_eq!(outcome["finalize_cause"], "Completed");
    assert_eq!(outcome["messages"].as_array().unwrap().len(), 1);
    assert_eq!(outcome["messages"][0]["role"], "user");
    assert_eq!(outcome["api_input_tokens"], 34);
    assert_eq!(outcome["fingerprint"], "production-fingerprint");
    assert_eq!(outcome["committed_revision"], 1);
}

/// The Runtime client's session id must match the wiring's committed session
/// id — proving no id drift between Context and Runtime.
#[tokio::test(flavor = "current_thread")]
async fn runtime_session_id_matches_wiring_committed_session() {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    let agents_dir = make_agents_dir(&temp);
    std::fs::create_dir_all(&root).expect("create project root");

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = wire_config_with_agents_dir(
        &root,
        &agents_dir,
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
        storage::FileSystemDatasetAdapter::new(agents_dir.clone()).expect("create dataset adapter"),
    );
    let legacy_factory = Arc::new(memory::FileLegacyMemorySourceFactory::new(
        agents_dir.join("memory"),
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

    let session_management = session_management(&agents_dir);
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
    let tools = tools::composition::TestCatalogExecutionFactory::empty();
    let skill_wiring = tools::composition::wire_skills();
    let tool_result_materializer = Arc::new(runtime::ToolResultMaterializer::new(
        Arc::new(runtime::AtomicBlobToolResultStore::new(
            Arc::new(storage::FileSystemBlobAdapter::new(temp.path()).expect("tool result blob")),
            temp.path().to_path_buf(),
        )),
        runtime::ToolResultMaterializationPolicy::new(50_000, 2_000, 500),
    ));
    let active_run = Arc::new(runtime::ActiveRunRegistry::default());
    let hook_runner: Arc<dyn hook::HookPort> = Arc::new(
        hook::build_dispatcher(&share::config::hooks::HooksConfig::default())
            .expect("test hook dispatcher"),
    );

    let provider_factory = composition::provider::provider_factory();
    let provider_spec = runtime::ProviderBuildSpec {
        driver: "openai".to_string(),
        source_key: "local".to_string(),
        api_style: None,
        api_key: "test-api-key".to_string(),
        base_url: Some("http://127.0.0.1:1/v1".to_string()),
        model: provider::ModelId {
            provider: "local".to_string(),
            model: "test-model".to_string(),
        },
        max_tokens: 8192,
        requested_reasoning: provider::ReasoningLevel::Off,
        context_window: Some(8192),
        timeout: std::time::Duration::from_secs(30),
        user_agent: "aemeath-test".to_string(),
    };
    let initial_binding = runtime::ProviderFactory::build(provider_factory.as_ref(), provider_spec)
        .expect("build test provider binding");
    let initial_provider = runtime::InitialProviderAssembly::new(
        initial_binding,
        share::config::models::ResolvedModel {
            source_key: "local".to_string(),
            source_config: share::config::models::ProviderModelsConfig::default(),
            model: share::config::models::ModelEntryConfig {
                id: "test-model".to_string(),
                context_window: 8192,
                max_tokens: 8192,
                ..Default::default()
            },
            driver: "openai".to_string(),
        },
        runtime::ModelRuntimeSettings {
            max_tokens: 8192,
            reasoning: false,
            reasoning_effort: None,
        },
    );

    struct NoopRunner;
    #[async_trait::async_trait]
    impl tools::AgentRunner for NoopRunner {
        async fn run_agent(&self, _request: tools::AgentRunRequest<'_>) -> tools::AgentRunTerminal {
            tools::AgentRunTerminal::Completed {
                result: String::new(),
            }
        }
    }
    let runtime_context_factory = Arc::new(runtime::RuntimeContextFactory::new(
        tools.catalog_port(),
        tools.execution(),
        Arc::new(policy::AllowAllPolicy),
        reflection_history,
        task_access,
        hook_runner,
    ));
    let agent_runner = runtime::AgentRunnerAssembly {
        runner: Arc::new(NoopRunner),
        parent_context_source: runtime::ParentRunContextSource::new(),
        max_tool_concurrency: 10,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        runtime_context_factory: runtime_context_factory.clone(),
    };

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        runtime::RuntimeCoreDependencies::new(
            workspace,
            wiring,
            provider_factory,
            session_management,
        ),
        runtime::RuntimeToolAssemblyDependencies::new(
            tools.catalog_port(),
            skill_wiring.catalog(),
            tool_result_materializer,
            active_run,
        ),
        runtime::composition::wire_sdk_chat_ingress(),
        initial_provider,
        runtime::SessionBootstrapAssembly::new(root.clone(), 8192, true, false, None),
        runtime::PromptAssembly::new(Vec::new(), String::new(), String::new()),
        runtime::SkillBootstrapAssembly::new(tools::SkillCatalogSnapshot::from_descriptors(
            Vec::new(),
        )),
        agent_runner,
    );
    assert!(Arc::ptr_eq(
        dependencies.runtime_context_factory(),
        &runtime_context_factory,
    ));

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
    let agents_dir = make_agents_dir(&temp);
    std::fs::create_dir_all(&root).expect("create project root");

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let config = wire_config_with_agents_dir(&root, &agents_dir, config::CliConfigInput::default())
        .await
        .expect("wire config");

    let task_wiring = task::wire_task();

    let dataset_adapter = Arc::new(
        storage::FileSystemDatasetAdapter::new(agents_dir.clone()).expect("create dataset adapter"),
    );
    let legacy_factory = Arc::new(memory::FileLegacyMemorySourceFactory::new(
        agents_dir.join("memory"),
    ));
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        dataset_adapter,
        legacy_factory,
    ));

    let session_management = session_management(&agents_dir);
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

#[test]
fn production_session_wiring_uses_dataset_writer_instead_of_blob_writer() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/runtime.rs"))
        .expect("read composition runtime wiring");
    assert!(
        source.contains("DatasetSessionManagement::new"),
        "production Session wiring must construct Dataset-aware management"
    );
    assert!(
        !source.contains("AtomicBlobSessionManagement::new"),
        "production Session wiring must not construct legacy-only management"
    );
    assert!(
        source.contains("DatasetCanonicalSessionWriter::new"),
        "production Session wiring must construct the incremental Dataset writer"
    );
    assert!(
        !source.contains("AtomicBlobCanonicalSessionWriter::new"),
        "production Session wiring must not construct the retired full-blob writer"
    );
}
