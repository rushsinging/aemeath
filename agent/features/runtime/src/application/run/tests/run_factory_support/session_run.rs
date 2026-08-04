use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::application::interaction::port::{InteractionBridge, InteractionPort};
use crate::application::run::context_factory::RuntimeContextFactory;
use crate::application::run::creation::{
    RunCreationError, RunCreationRequest, RunInstance, SessionRunBindings, SessionState,
};
use crate::application::run::factory::RunFactory;
use crate::domain::agent_run::RunSpec;
use crate::ports::{ContextPort, ProviderBinding};

struct FixedMainContextFactory {
    context: Arc<dyn ContextPort>,
}

impl context::ports::MainContextFactory for FixedMainContextFactory {
    fn build(
        &self,
        _session: Arc<RwLock<Arc<context::session::CanonicalSession>>>,
        _task_persist: Arc<dyn task::TaskPersist>,
        _workspace_persist: Arc<dyn project::WorkspacePersist>,
        _memory: Arc<RwLock<Arc<dyn memory::MemoryPort>>>,
        _mutation_gate: Arc<tokio::sync::Mutex<()>>,
    ) -> Arc<dyn ContextPort> {
        self.context.clone()
    }
}

use super::doubles::{
    fake_provider_binding, FakeContextPort, FakeHookPort, FakePolicyPort, FakeReflectionHistory,
    FakeToolCatalog, FakeToolExecution, RecordingEventSink,
};

pub(crate) struct SessionRunFixture {
    context_factory: Arc<RuntimeContextFactory>,
    session_state: SessionState,
    session_bindings: SessionRunBindings,
    context_port: Arc<dyn ContextPort>,
    memory: Arc<dyn memory::api::MemoryPort>,
    provider: Arc<ProviderBinding>,
    interaction: Arc<dyn InteractionPort>,
    reasoning: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
    event_sink: RecordingEventSink,
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    policy: Arc<dyn crate::ports::PolicyPort>,
    hooks: Arc<dyn hook::HookPort>,
    workspace: crate::application::run::workspace::RuntimeWorkspaceAccess,
}
impl SessionRunFixture {
    pub(crate) fn builder() -> SessionRunFixtureBuilder {
        SessionRunFixtureBuilder::new()
    }

    pub(crate) fn context_factory(&self) -> Arc<RuntimeContextFactory> {
        self.context_factory.clone()
    }

    pub(crate) fn use_snapshot_hooks(&mut self) {
        Arc::get_mut(&mut self.context_factory)
            .expect("fixture owns its context factory")
            .use_snapshot_hooks_for_test();
    }

    pub(crate) fn session_revision(&self) -> u64 {
        self.session_state.snapshot_for_run().revision()
    }

    pub(crate) fn session_snapshot(&self) -> crate::application::run::creation::SessionSnapshot {
        self.session_state.snapshot_for_run()
    }

    pub(crate) fn committed_context(&self) -> &Arc<dyn ContextPort> {
        &self.context_port
    }

    pub(crate) fn memory(&self) -> &Arc<dyn memory::api::MemoryPort> {
        &self.memory
    }

    pub(crate) fn provider(&self) -> &Arc<ProviderBinding> {
        &self.provider
    }

    pub(crate) fn interaction(&self) -> &Arc<dyn InteractionPort> {
        &self.interaction
    }

    pub(crate) fn reasoning(&self) -> &Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>> {
        &self.reasoning
    }

    pub(crate) fn event_sink(&self) -> &RecordingEventSink {
        &self.event_sink
    }

    pub(crate) fn tool_catalog(&self) -> &Arc<dyn tools::ToolCatalogPort> {
        &self.tool_catalog
    }

    pub(crate) fn tool_execution(&self) -> &Arc<dyn tools::ToolExecutionPort> {
        &self.tool_execution
    }

    pub(crate) fn policy(&self) -> &Arc<dyn crate::ports::PolicyPort> {
        &self.policy
    }

    pub(crate) fn hooks(&self) -> &Arc<dyn hook::HookPort> {
        &self.hooks
    }

    pub(crate) fn workspace(&self) -> &crate::application::run::workspace::RuntimeWorkspaceAccess {
        &self.workspace
    }

    pub(crate) fn create(&self, spec: RunSpec) -> Result<RunInstance, RunCreationError> {
        let request = RunCreationRequest::new(spec, self.session_state.snapshot_for_run(), None)?;
        RunFactory::for_session(self.context_factory.clone(), self.session_bindings.clone())
            .create(request)
    }
}

impl Default for SessionRunFixture {
    fn default() -> Self {
        SessionRunFixtureBuilder::new().build()
    }
}

pub(crate) struct SessionRunFixtureBuilder {
    context_port: Arc<dyn ContextPort>,
    provider: Arc<ProviderBinding>,
    interaction: Arc<dyn InteractionPort>,
    reasoning: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
    event_sink: RecordingEventSink,
    event_sink_handle: Option<crate::application::loop_engine::chat::ChatEventSinkHandle>,
    hooks: Arc<dyn hook::HookPort>,
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    policy: Arc<dyn crate::ports::PolicyPort>,
    context_factory: Option<Arc<RuntimeContextFactory>>,
    config: share::config::domain::snapshot::ConfigSnapshot,
    session_id: String,
    workspace_root: PathBuf,
}

impl SessionRunFixtureBuilder {
    pub(crate) fn new() -> Self {
        Self {
            context_port: Arc::new(FakeContextPort),
            provider: fake_provider_binding(),
            interaction: Arc::new(InteractionBridge::new()),
            reasoning: Arc::new(std::sync::Mutex::new(
                share::reasoning::ReasoningLevel::Medium,
            )),
            event_sink: RecordingEventSink::default(),
            event_sink_handle: None,
            hooks: Arc::new(FakeHookPort),
            tool_catalog: Arc::new(FakeToolCatalog),
            tool_execution: Arc::new(FakeToolExecution),
            policy: Arc::new(FakePolicyPort),
            context_factory: None,
            config: share::config::domain::snapshot::ConfigSnapshot::new_with_revision(
                share::config::domain::snapshot::ConfigRevision::new(1),
                share::config::Config::default(),
            ),
            session_id: "test-session".to_string(),
            workspace_root: std::env::temp_dir().join(format!(
                "aemeath-session-run-fixture-{}",
                uuid::Uuid::now_v7()
            )),
        }
    }

    pub(crate) fn with_context_port(mut self, context: Arc<dyn ContextPort>) -> Self {
        self.context_port = context;
        self
    }

    pub(crate) fn with_provider_binding(mut self, provider: Arc<ProviderBinding>) -> Self {
        self.provider = provider;
        self
    }

    pub(crate) fn with_interaction(mut self, interaction: Arc<dyn InteractionPort>) -> Self {
        self.interaction = interaction;
        self
    }

    pub(crate) fn with_reasoning(
        mut self,
        reasoning: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
    ) -> Self {
        self.reasoning = reasoning;
        self
    }

    pub(crate) fn with_event_sink(mut self, event_sink: RecordingEventSink) -> Self {
        self.event_sink = event_sink;
        self
    }

    pub(crate) fn with_event_sink_handle(
        mut self,
        event_sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    ) -> Self {
        self.event_sink_handle = Some(event_sink);
        self
    }

    pub(crate) fn with_tool_catalog(
        mut self,
        tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    ) -> Self {
        self.tool_catalog = tool_catalog;
        self
    }

    pub(crate) fn with_tool_execution(
        mut self,
        tool_execution: Arc<dyn tools::ToolExecutionPort>,
    ) -> Self {
        self.tool_execution = tool_execution;
        self
    }

    pub(crate) fn with_policy(mut self, policy: Arc<dyn crate::ports::PolicyPort>) -> Self {
        self.policy = policy;
        self
    }

    pub(crate) fn with_context_factory(
        mut self,
        context_factory: Arc<RuntimeContextFactory>,
    ) -> Self {
        self.context_factory = Some(context_factory);
        self
    }

    pub(crate) fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = session_id.into();
        self
    }

    pub(crate) fn with_config(
        mut self,
        config: share::config::domain::snapshot::ConfigSnapshot,
    ) -> Self {
        self.config = config;
        self
    }

    pub(crate) fn build(self) -> SessionRunFixture {
        std::fs::create_dir_all(&self.workspace_root).expect("create fixture workspace");
        let session_state = SessionState::new(
            self.session_id.clone(),
            self.workspace_root.clone(),
            format!(
                "{}/{}",
                self.provider.model.provider, self.provider.model.model
            ),
            self.config.clone(),
        );
        let session_snapshot = session_state.snapshot_for_run();
        let workspace = project::wire_production_workspace(self.workspace_root.clone())
            .expect("wire fixture workspace")
            .into_views();
        let workspace_access =
            crate::application::run::workspace::RuntimeWorkspaceAccess::new(workspace.clone());
        let task_store = Arc::new(task::TaskStore::new());
        let config_service = Arc::new(config::ConfigAppService::new(Some(&self.workspace_root)));
        let now = chrono::Utc::now().to_rfc3339();
        let memory: Arc<dyn memory::api::MemoryPort> = Arc::new(memory::NoOpMemory);
        let wiring = Arc::new(context::MainSessionWiring::build(
            context::MainSessionWiringBuilder {
                workspace_read: workspace.read(),
                workspace_persist: workspace.persist(),
                task_persist: task_store.clone(),
                config_reader: config_service.clone(),
                config_participant: config_service,
                memory_opener: Box::new(context::test_support::InMemoryTestOpener),
                session_management: Arc::new(context::test_support::UnavailableSessionManagement),
                initial_session: context::session::CanonicalSession {
                    id: session_snapshot.session_id().to_string(),
                    chats: Vec::new(),
                    created_at: now.clone(),
                    updated_at: now,
                    metadata: Default::default(),
                    tasks: context::session::SnapshotState::Missing,
                    workspace: context::session::SnapshotState::Captured(
                        workspace.persist().snapshot(),
                    ),
                    revision: session_snapshot.revision(),
                    compact: None,
                    run_slices: Vec::new().into(),
                    committed_steps: Default::default(),
                    skill_load_records: Vec::new(),
                },
                initial_memory: memory.clone(),
                context_factory: Arc::new(FixedMainContextFactory {
                    context: self.context_port.clone(),
                }),
            },
        ));
        let committed_context = wiring.committed_context();
        let committed_memory = wiring.committed_memory();
        let event_sink_handle = self
            .event_sink_handle
            .unwrap_or_else(|| self.event_sink.handle());
        let session_bindings = SessionRunBindings::new(
            wiring,
            self.provider.clone(),
            self.interaction.clone(),
            self.reasoning.clone(),
            event_sink_handle,
        );
        let tool_catalog = self.tool_catalog;
        let tool_execution = self.tool_execution;
        let policy = self.policy;
        let context_factory = self.context_factory.unwrap_or_else(|| {
            Arc::new(RuntimeContextFactory::new(
                tool_catalog.clone(),
                tool_execution.clone(),
                policy.clone(),
                Arc::new(FakeReflectionHistory),
                task_store,
                self.hooks.clone(),
            ))
        });
        SessionRunFixture {
            context_factory,
            session_state,
            session_bindings,
            context_port: committed_context,
            memory: committed_memory,
            provider: self.provider,
            interaction: self.interaction,
            reasoning: self.reasoning,
            event_sink: self.event_sink,
            tool_catalog,
            tool_execution,
            policy,
            hooks: self.hooks,
            workspace: workspace_access,
        }
    }
}
