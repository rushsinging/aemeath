use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use runtime::ProviderFactory;
use sdk::{AgentClient, MemoryConfigView, SdkError};

use crate::runtime::{AgentArgs, AgentClientImpl};
use logging::{LoggingOutputMode, LoggingSettings, UnifiedLogger};
use share::config::domain::snapshot::ConfigSnapshot;
use std::path::Path;

use async_trait::async_trait;
use config::connect::{
    ConnectAppService, ConnectCommitError, ConnectCommitPort, ConnectCommitReceipt,
    ConnectCommitRequest, ConnectError, ConnectOrigin, ConnectView, ExistingProviderSnapshot,
};

pub struct ConnectFacade {
    service: Arc<ConnectAppService>,
    store: Arc<dyn config::GlobalConfigConnectStore>,
}

impl ConnectFacade {
    pub fn new(
        service: Arc<ConnectAppService>,
        store: Arc<dyn config::GlobalConfigConnectStore>,
    ) -> Arc<Self> {
        Arc::new(Self { service, store })
    }

    pub async fn start(
        &self,
        origin: sdk::ConnectOrigin,
        existing_provider: Option<ExistingProviderSnapshot>,
    ) -> Result<sdk::ConnectView, sdk::SdkError> {
        let document = self
            .store
            .load_global_document()
            .await
            .map_err(|error| sdk::SdkError::Internal(error.to_string()))?
            .ok_or_else(|| sdk::SdkError::Internal("全局配置不存在".to_string()))?;
        let view = self
            .service
            .start_connect(config_origin(origin), document.revision, existing_provider)
            .await;
        Ok(sdk_view(view))
    }

    pub async fn apply(
        &self,
        session_id: sdk::ConnectSessionId,
        revision: sdk::ConnectRevision,
        command: sdk::ConnectCommand,
    ) -> Result<sdk::ConnectView, sdk::SdkError> {
        let command = match command {
            sdk::ConnectCommand::SelectProvider { source } => {
                let existing_provider = self.existing_provider(&source).await?;
                if let Some(existing_provider) = existing_provider {
                    self.service
                        .attach_existing_provider(
                            config::connect::ConnectSessionId::from_transport_str(&session_id.0)
                                .map_err(sdk::SdkError::Internal)?,
                            config::connect::ConnectRevision::from_value(revision.0),
                            existing_provider,
                        )
                        .await
                        .map_err(connect_sdk_error)?;
                }
                sdk::ConnectCommand::SelectProvider { source }
            }
            other => other,
        };
        let session_id = config::connect::ConnectSessionId::from_transport_str(&session_id.0)
            .map_err(sdk::SdkError::Internal)?;
        self.service
            .apply(
                session_id,
                config::connect::ConnectRevision::from_value(revision.0),
                config_command(command)?,
            )
            .await
            .map(sdk_view)
            .map_err(connect_sdk_error)
    }

    async fn existing_provider(
        &self,
        source: &str,
    ) -> Result<Option<ExistingProviderSnapshot>, sdk::SdkError> {
        let document = self
            .store
            .load_global_document()
            .await
            .map_err(|error| sdk::SdkError::Internal(error.to_string()))?;
        Ok(document.and_then(|document| existing_provider_snapshot(&document.value, source)))
    }

    pub async fn cancel(
        &self,
        session_id: sdk::ConnectSessionId,
        revision: sdk::ConnectRevision,
    ) -> Result<sdk::ConnectView, sdk::SdkError> {
        let session_id = config::connect::ConnectSessionId::from_transport_str(&session_id.0)
            .map_err(sdk::SdkError::Internal)?;
        self.service
            .cancel(
                session_id,
                config::connect::ConnectRevision::from_value(revision.0),
            )
            .await
            .map(sdk_view)
            .map_err(connect_sdk_error)
    }

    pub async fn view(
        &self,
        session_id: sdk::ConnectSessionId,
    ) -> Result<Option<sdk::ConnectView>, sdk::SdkError> {
        let session_id = config::connect::ConnectSessionId::from_transport_str(&session_id.0)
            .map_err(sdk::SdkError::Internal)?;
        Ok(self.service.view(session_id).await.map(sdk_view))
    }
}

#[async_trait::async_trait]
impl sdk::ConnectClient for ConnectFacade {
    async fn start_connect(
        &self,
        origin: sdk::ConnectOrigin,
    ) -> Result<sdk::ConnectView, SdkError> {
        self.start(origin, None).await
    }

    async fn apply_connect(
        &self,
        session_id: sdk::ConnectSessionId,
        revision: sdk::ConnectRevision,
        command: sdk::ConnectCommand,
    ) -> Result<sdk::ConnectView, SdkError> {
        self.apply(session_id, revision, command).await
    }

    async fn cancel_connect(
        &self,
        session_id: sdk::ConnectSessionId,
        revision: sdk::ConnectRevision,
    ) -> Result<sdk::ConnectView, SdkError> {
        self.cancel(session_id, revision).await
    }

    async fn connect_view(
        &self,
        session_id: sdk::ConnectSessionId,
    ) -> Result<Option<sdk::ConnectView>, SdkError> {
        self.view(session_id).await
    }
}

pub struct GlobalConnectCommitAdapter {
    store: Arc<dyn config::GlobalConfigConnectStore>,
}

impl GlobalConnectCommitAdapter {
    pub fn new(store: Arc<dyn config::GlobalConfigConnectStore>) -> Arc<Self> {
        Arc::new(Self { store })
    }
}

#[async_trait]
impl ConnectCommitPort for GlobalConnectCommitAdapter {
    async fn commit(
        &self,
        request: ConnectCommitRequest,
    ) -> Result<ConnectCommitReceipt, ConnectCommitError> {
        let receipt = self
            .store
            .compare_and_swap(request.expected_global_revision, request.draft)
            .await
            .map_err(map_store_error)?;
        let applied_revision = stable_revision(receipt.revision.as_str());
        Ok(ConnectCommitReceipt { applied_revision })
    }
}

pub struct FirstChatConnectBootstrap {
    pub connect: Arc<dyn sdk::ConnectClient>,
    store: Arc<dyn config::GlobalConfigConnectStore>,
    receipt: config::BootstrapConfigReceipt,
}

impl FirstChatConnectBootstrap {
    pub async fn rollback(self) -> Result<(), SdkError> {
        self.store
            .rollback_bootstrap(self.receipt)
            .await
            .map_err(global_connect_store_sdk_error)
    }
}

pub async fn prepare_first_chat(
    interactive: bool,
) -> Result<Option<FirstChatConnectBootstrap>, SdkError> {
    let agents_dir = share::config::paths::global_agents_dir();
    prepare_first_chat_with_agents_dir(&agents_dir, interactive).await
}

pub async fn prepare_first_chat_with_agents_dir(
    agents_dir: &Path,
    interactive: bool,
) -> Result<Option<FirstChatConnectBootstrap>, SdkError> {
    let store: Arc<dyn config::GlobalConfigConnectStore> = Arc::new(
        config::FilesystemGlobalConfigConnectStore::new(agents_dir.to_path_buf()),
    );
    if store
        .load_global_document()
        .await
        .map_err(global_connect_store_sdk_error)?
        .is_some()
    {
        return Ok(None);
    }
    if !interactive {
        return Err(SdkError::Init(
            "缺少全局配置；请在交互终端运行 `aemeath connect`".to_string(),
        ));
    }
    let receipt = store
        .create_complete_default()
        .await
        .map_err(global_connect_store_sdk_error)?;
    Ok(Some(FirstChatConnectBootstrap {
        connect: wire_connect_with_store(store.clone()),
        store,
        receipt,
    }))
}

pub struct ConnectBootstrap {
    pub connect: Arc<dyn sdk::ConnectClient>,
}

pub async fn build_connect_bootstrap() -> Result<ConnectBootstrap, SdkError> {
    let agents_dir = share::config::paths::global_agents_dir();
    build_connect_bootstrap_with_agents_dir(&agents_dir).await
}

pub async fn build_connect_bootstrap_with_agents_dir(
    agents_dir: &Path,
) -> Result<ConnectBootstrap, SdkError> {
    let store: Arc<dyn config::GlobalConfigConnectStore> = Arc::new(
        config::FilesystemGlobalConfigConnectStore::new(agents_dir.to_path_buf()),
    );
    if store
        .load_global_document()
        .await
        .map_err(global_connect_store_sdk_error)?
        .is_none()
    {
        store
            .create_complete_default()
            .await
            .map_err(global_connect_store_sdk_error)?;
    }
    Ok(ConnectBootstrap {
        connect: wire_connect_with_store(store),
    })
}

fn wire_connect(agents_dir: &std::path::Path) -> Arc<ConnectFacade> {
    let store: Arc<dyn config::GlobalConfigConnectStore> = Arc::new(
        config::FilesystemGlobalConfigConnectStore::new(agents_dir.to_path_buf()),
    );
    wire_connect_with_store(store)
}

fn wire_connect_with_store(store: Arc<dyn config::GlobalConfigConnectStore>) -> Arc<ConnectFacade> {
    let commit = GlobalConnectCommitAdapter::new(store.clone());
    let service = Arc::new(
        ConnectAppService::builder()
            .with_catalog(config::catalog::PROVIDER_CATALOG)
            .with_probe(crate::provider::ProviderProbeAdapter::new())
            .with_commit(commit)
            .with_system(config::ports::SystemInformation {
                os_name: std::env::consts::OS.to_string(),
                os_version: None,
                arch: std::env::consts::ARCH.to_string(),
            })
            .build(),
    );
    ConnectFacade::new(service, store)
}

fn existing_provider_snapshot(
    document: &serde_json::Value,
    source: &str,
) -> Option<ExistingProviderSnapshot> {
    let provider = document.get("models")?.get("providers")?.get(source)?;
    let model = provider
        .get("models")
        .and_then(serde_json::Value::as_array)
        .and_then(|models| models.first());
    Some(ExistingProviderSnapshot::from_provider_config(
        source,
        provider
            .get("baseUrl")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
        provider.get("apiKey").and_then(serde_json::Value::as_str),
        provider.get("driver").and_then(serde_json::Value::as_str),
        model
            .and_then(|model| model.get("id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
        model
            .and_then(|model| model.get("contextWindow"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_default(),
        model
            .and_then(|model| model.get("max_tokens").or_else(|| model.get("maxTokens")))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or_default(),
    ))
}

fn global_connect_store_sdk_error(error: config::GlobalConfigStoreError) -> SdkError {
    SdkError::Init(format!("全局配置初始化失败：{error}"))
}

fn map_store_error(error: config::GlobalConfigStoreError) -> ConnectCommitError {
    match error {
        config::GlobalConfigStoreError::Conflict { .. } => {
            ConnectCommitError::PersistConflict { expected: 0 }
        }
        config::GlobalConfigStoreError::AlreadyExists
        | config::GlobalConfigStoreError::InvalidDocument(_)
        | config::GlobalConfigStoreError::InvalidDraft(_)
        | config::GlobalConfigStoreError::RollbackRefused => {
            ConnectCommitError::internal(error.to_string())
        }
        config::GlobalConfigStoreError::Io(message) => ConnectCommitError::io(message),
    }
}

fn stable_revision(value: &str) -> u64 {
    let bytes = value.as_bytes();
    let mut folded = 0_u64;
    for byte in bytes.iter().take(16) {
        folded = folded.wrapping_mul(257).wrapping_add(u64::from(*byte));
    }
    folded
}

fn config_origin(origin: sdk::ConnectOrigin) -> ConnectOrigin {
    match origin {
        sdk::ConnectOrigin::ExplicitCommand => ConnectOrigin::ExplicitCommand,
        sdk::ConnectOrigin::FirstChatBootstrap => ConnectOrigin::FirstChatBootstrap,
    }
}

fn config_command(
    command: sdk::ConnectCommand,
) -> Result<config::connect::ConnectCommand, sdk::SdkError> {
    use config::connect::ConnectCommand as Target;
    use sdk::ConnectCommand as Source;
    Ok(match command {
        Source::SelectProvider { source } => Target::SelectProvider {
            source: config::catalog::find_by_source(&source)
                .ok_or_else(|| sdk::SdkError::Internal("未知 Provider".to_string()))?
                .source,
        },
        Source::ConfirmOverwrite => Target::ConfirmOverwrite,
        Source::RejectOverwrite => Target::RejectOverwrite,
        Source::SetEndpoint { base_url } => Target::SetEndpoint { base_url },
        Source::SetCredential { api_key } => Target::SetCredential { api_key },
        Source::SetProviderUserAgent { raw } => Target::SetProviderUserAgent { raw },
        Source::SelectRecommendedModel { index } => Target::SelectRecommendedModel { index },
        Source::EnterCustomModel => Target::EnterCustomModel,
        Source::SetCustomModel {
            model_id,
            context_window,
            max_tokens,
        } => Target::SetCustomModel {
            model_id,
            context_window,
            max_tokens,
        },
        Source::SetGlobalDefault { set_as_default } => Target::SetGlobalDefault { set_as_default },
        Source::SkipProbe => Target::SkipProbe,
        Source::BeginProbe => Target::BeginProbe,
        Source::ContinueAfterProbe => Target::ContinueAfterProbe,
        Source::EditAfterProbeFailure => Target::EditAfterProbeFailure,
        Source::ConfirmSave => Target::ConfirmSave,
    })
}

fn sdk_view(view: ConnectView) -> sdk::ConnectView {
    sdk::ConnectView {
        session_id: sdk::ConnectSessionId(view.session_id.to_transport_string()),
        revision: sdk::ConnectRevision(view.revision.value()),
        stage: sdk_stage(view.stage),
        origin: match view.origin {
            ConnectOrigin::ExplicitCommand => sdk::ConnectOrigin::ExplicitCommand,
            ConnectOrigin::FirstChatBootstrap => sdk::ConnectOrigin::FirstChatBootstrap,
        },
        catalog: config::catalog::PROVIDER_CATALOG
            .iter()
            .map(|entry| sdk::ConnectProviderOption {
                source: entry.source.as_str().to_string(),
                driver: entry.driver.as_str().to_string(),
                default_base_url: entry
                    .default_endpoint
                    .map(|endpoint| endpoint.url.to_string())
                    .unwrap_or_default(),
                recommended_models: entry
                    .recommended_models
                    .iter()
                    .map(|model| sdk::ConnectRecommendedModelOption {
                        model_id: model.model_id.to_string(),
                        context_window: model.context_window,
                        max_tokens: model.max_tokens,
                    })
                    .collect(),
            })
            .collect(),
        draft: sdk::ConnectDraftView {
            source: view.draft.source.map(|value| value.as_str().to_string()),
            driver: view.draft.driver.map(|value| value.as_str().to_string()),
            base_url: view.draft.base_url,
            has_api_key: view.draft.has_api_key,
            provider_user_agent: view.draft.provider_user_agent,
            model: view.draft.model.map(|model| sdk::ConnectModelDraftView {
                model_id: model.model_id,
                context_window: model.context_window,
                max_tokens: model.max_tokens,
            }),
            set_global_default: view.draft.set_global_default,
        },
        existing_provider: view.existing_provider.map(|provider| {
            sdk::ConnectExistingProviderView {
                source: provider.source,
                driver: provider.driver,
                base_url: provider.base_url,
                has_api_key: matches!(
                    provider.api_key_status,
                    config::connect::ExistingCredentialStatus::Present
                ),
                model_id: provider.model_id,
            }
        }),
        available_actions: view.available_actions.into_iter().map(sdk_action).collect(),
        probe_status: view.probe_status.map(sdk_probe_status),
        last_error: view.last_error.map(sdk_error_view),
        terminal: view.terminal.map(|outcome| match outcome {
            config::connect::ConnectOutcome::Completed { applied_revision } => {
                sdk::ConnectOutcome::Completed { applied_revision }
            }
            config::connect::ConnectOutcome::Cancelled => sdk::ConnectOutcome::Cancelled,
        }),
    }
}

fn sdk_stage(stage: config::connect::ConnectStage) -> sdk::ConnectStage {
    use config::connect::ConnectStage as Source;
    match stage {
        Source::SelectProvider => sdk::ConnectStage::SelectProvider,
        Source::ConfirmOverwrite => sdk::ConnectStage::ConfirmOverwrite,
        Source::EditEndpoint => sdk::ConnectStage::EditEndpoint,
        Source::EditCredential => sdk::ConnectStage::EditCredential,
        Source::EditUserAgent => sdk::ConnectStage::EditUserAgent,
        Source::SelectModel => sdk::ConnectStage::SelectModel,
        Source::EditCustomModel => sdk::ConnectStage::EditCustomModel,
        Source::ChooseGlobalDefault => sdk::ConnectStage::ChooseGlobalDefault,
        Source::ChooseProbe => sdk::ConnectStage::ChooseProbe,
        Source::Probing => sdk::ConnectStage::Probing,
        Source::Review => sdk::ConnectStage::Review,
        Source::Saving => sdk::ConnectStage::Saving,
        Source::Completed => sdk::ConnectStage::Completed,
        Source::Cancelled => sdk::ConnectStage::Cancelled,
    }
}

fn sdk_action(action: config::connect::AvailableAction) -> sdk::ConnectAvailableAction {
    use config::connect::AvailableAction as Source;
    match action {
        Source::SelectProvider => sdk::ConnectAvailableAction::SelectProvider,
        Source::ConfirmOverwrite => sdk::ConnectAvailableAction::ConfirmOverwrite,
        Source::RejectOverwrite => sdk::ConnectAvailableAction::RejectOverwrite,
        Source::SetEndpoint => sdk::ConnectAvailableAction::SetEndpoint,
        Source::SetCredential => sdk::ConnectAvailableAction::SetCredential,
        Source::SetProviderUserAgent => sdk::ConnectAvailableAction::SetProviderUserAgent,
        Source::SelectRecommendedModel => sdk::ConnectAvailableAction::SelectRecommendedModel,
        Source::EnterCustomModel => sdk::ConnectAvailableAction::EnterCustomModel,
        Source::SetCustomModel => sdk::ConnectAvailableAction::SetCustomModel,
        Source::SetGlobalDefault => sdk::ConnectAvailableAction::SetGlobalDefault,
        Source::SkipProbe => sdk::ConnectAvailableAction::SkipProbe,
        Source::BeginProbe => sdk::ConnectAvailableAction::BeginProbe,
        Source::ContinueAfterProbe => sdk::ConnectAvailableAction::ContinueAfterProbe,
        Source::EditAfterProbeFailure => sdk::ConnectAvailableAction::EditAfterProbeFailure,
        Source::ConfirmSave => sdk::ConnectAvailableAction::ConfirmSave,
        Source::RetrySave => sdk::ConnectAvailableAction::RetrySave,
        Source::Cancel => sdk::ConnectAvailableAction::Cancel,
    }
}

fn sdk_probe_status(status: config::connect::ProbeStatusView) -> sdk::ConnectProbeStatus {
    match status {
        config::connect::ProbeStatusView::NotRun => sdk::ConnectProbeStatus::NotRun,
        config::connect::ProbeStatusView::Running => sdk::ConnectProbeStatus::Running,
        config::connect::ProbeStatusView::Success { latency_ms } => {
            sdk::ConnectProbeStatus::Success { latency_ms }
        }
        config::connect::ProbeStatusView::Failed { kind, message } => {
            sdk::ConnectProbeStatus::Failed {
                kind: sdk_probe_error_kind(kind),
                message,
            }
        }
    }
}

fn sdk_probe_error_kind(kind: config::ports::ProviderProbeErrorKind) -> sdk::ConnectProbeErrorKind {
    match kind {
        config::ports::ProviderProbeErrorKind::Cancelled => sdk::ConnectProbeErrorKind::Cancelled,
        config::ports::ProviderProbeErrorKind::Timeout => sdk::ConnectProbeErrorKind::Timeout,
        config::ports::ProviderProbeErrorKind::Authentication => {
            sdk::ConnectProbeErrorKind::Authentication
        }
        config::ports::ProviderProbeErrorKind::Endpoint => sdk::ConnectProbeErrorKind::Endpoint,
        config::ports::ProviderProbeErrorKind::Model => sdk::ConnectProbeErrorKind::Model,
        config::ports::ProviderProbeErrorKind::Protocol => sdk::ConnectProbeErrorKind::Protocol,
        config::ports::ProviderProbeErrorKind::Internal => sdk::ConnectProbeErrorKind::Internal,
    }
}

fn sdk_error_view(error: ConnectError) -> sdk::ConnectErrorView {
    sdk::ConnectErrorView {
        kind: match error {
            ConnectError::InvalidTransition { .. } => sdk::ConnectErrorKind::InvalidTransition,
            ConnectError::StaleRevision { .. } => sdk::ConnectErrorKind::StaleRevision,
            ConnectError::Validation { .. } => sdk::ConnectErrorKind::Validation,
            ConnectError::CatalogUnavailable { .. } => sdk::ConnectErrorKind::CatalogUnavailable,
            ConnectError::ProbeFailed { .. } => sdk::ConnectErrorKind::ProbeFailed,
            ConnectError::PersistConflict { .. } => sdk::ConnectErrorKind::PersistConflict,
            ConnectError::PersistFailed { .. } => sdk::ConnectErrorKind::PersistFailed,
            ConnectError::PersistUnavailable => sdk::ConnectErrorKind::PersistUnavailable,
            ConnectError::InteractiveSetupRequired => {
                sdk::ConnectErrorKind::InteractiveSetupRequired
            }
            ConnectError::BootstrapRollbackRefused { .. } => {
                sdk::ConnectErrorKind::BootstrapRollbackRefused
            }
        },
        message: error.display_message(),
    }
}

fn connect_sdk_error(error: ConnectError) -> sdk::SdkError {
    sdk::SdkError::Internal(error.display_message())
}

pub type AgentClientHandle = Arc<dyn AgentClient>;

pub struct AgentClientBootstrap {
    pub client: AgentClientHandle,
    pub connect: Arc<dyn sdk::ConnectClient>,
    pub session_id: String,
    pub startup_resume: Option<sdk::SessionResumeView>,
    pub cwd: PathBuf,
    pub model_display: String,
    pub allow_all: bool,
    pub context_size: usize,
    pub thinking: bool,
    pub config_view: sdk::ConfigView,
    pub memory_config: MemoryConfigView,
    pub skill_snapshot: sdk::SkillsUpdatedEvent,
    pub command_catalog: Arc<dyn sdk::CommandCatalogPort>,
    pub command_router: Arc<dyn sdk::CommandRouterPort>,
    pub user_agent: String,
}

pub struct PreChatAgentClient {
    runtime: AgentClientImpl,
}

impl PreChatAgentClient {
    fn new(runtime: AgentClientImpl) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait]
impl AgentClient for PreChatAgentClient {
    fn cancel_current_run(&self, deadline: sdk::ControlDeadline) -> sdk::CancelCurrentRunOutcome {
        self.runtime.cancel_current_run(deadline)
    }

    fn reply_interaction(
        &self,
        request_id: &sdk::InteractionRequestId,
        reply: sdk::InteractionReply,
    ) -> sdk::InteractionCommandOutcome {
        self.runtime.reply_interaction(request_id, reply)
    }

    fn cancel_interaction(
        &self,
        request_id: &sdk::InteractionRequestId,
        reason: sdk::InteractionCancelReason,
    ) -> sdk::InteractionCommandOutcome {
        self.runtime.cancel_interaction(request_id, reason)
    }

    async fn config_view(&self) -> Result<sdk::ConfigView, SdkError> {
        self.runtime.config_view().await
    }

    async fn update_config(
        &self,
        update: sdk::ConfigUpdate,
    ) -> Result<sdk::ConfigUpdateResult, SdkError> {
        self.runtime.update_config(update).await
    }

    async fn chat(&self, input: sdk::ChatRequest) -> Result<sdk::ChatStream, SdkError> {
        self.runtime.chat(input).await
    }
}

pub fn agent_client_from_runtime(client: AgentClientImpl) -> AgentClientHandle {
    Arc::new(PreChatAgentClient::new(client))
}

pub struct FeatureGateways {
    pub provider: Arc<dyn ProviderFactory>,
    pub policy: Arc<dyn policy::PolicyPort>,
}

impl FeatureGateways {
    pub fn new(provider: Arc<dyn ProviderFactory>, policy: Arc<dyn policy::PolicyPort>) -> Self {
        Self { provider, policy }
    }

    pub fn wire_default(policy: Arc<dyn policy::PolicyPort>) -> Self {
        Self::new(crate::provider::provider_factory(), policy)
    }
}

struct ConfigPolicyModeSource {
    reader: Arc<dyn config::ConfigReader>,
}

impl policy::PolicyModeSource for ConfigPolicyModeSource {
    fn current_mode(&self) -> policy::PolicyMode {
        self.reader.committed_snapshot().permission_mode().into()
    }
}

fn configured_policy(config: &config::ConfigWiring) -> Arc<dyn policy::PolicyPort> {
    Arc::new(policy::ConfiguredPolicy::new(ConfigPolicyModeSource {
        reader: config.reader(),
    }))
}

fn cli_config_input(args: &AgentArgs) -> config::CliConfigInput {
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

fn wire_config_override_store(agents_dir: &Path) -> Result<config::NativeConfigStore, SdkError> {
    let blob = storage::api::file_system_blob(agents_dir.join("config-overrides"))
        .map_err(|error| SdkError::Init(format!("配置 override 存储初始化失败：{error}")))?;
    Ok(config::NativeConfigStore::new(blob))
}

fn logging_settings_from_snapshot(
    snapshot: &ConfigSnapshot,
    default_logs_dir: &Path,
    output_mode: LoggingOutputMode,
) -> LoggingSettings {
    LoggingSettings::new(
        snapshot.logging_level().to_string(),
        output_mode,
        snapshot
            .logs_dir()
            .map(PathBuf::from)
            .unwrap_or_else(|| default_logs_dir.to_path_buf()),
        snapshot.logging_max_bytes(),
        snapshot.logging_max_backups(),
        snapshot.logging_retention_days(),
    )
}

fn logging_settings_from_bootstrap(
    snapshot: &ConfigSnapshot,
    default_logs_dir: &Path,
    output_mode: sdk::LoggingOutputMode,
) -> LoggingSettings {
    let output_mode = match output_mode {
        sdk::LoggingOutputMode::File => LoggingOutputMode::File,
        sdk::LoggingOutputMode::Stderr => LoggingOutputMode::Stderr,
    };
    logging_settings_from_snapshot(snapshot, default_logs_dir, output_mode)
}

static LOGGING_INIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoggingInitDecision {
    Initialize,
    AlreadyInitialized,
}

fn logging_init_decision(
    current: Option<LoggingOutputMode>,
    requested: LoggingOutputMode,
) -> Result<LoggingInitDecision, String> {
    match current {
        None => Ok(LoggingInitDecision::Initialize),
        Some(existing) if existing == requested => Ok(LoggingInitDecision::AlreadyInitialized),
        Some(existing) => Err(format!(
            "logging already initialized with output mode {existing:?}; requested {requested:?} conflicts"
        )),
    }
}

fn init_logging(
    snapshot: &ConfigSnapshot,
    output_mode: sdk::LoggingOutputMode,
    default_logs_dir: &Path,
) -> Result<(), String> {
    let _guard = LOGGING_INIT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "日志初始化锁已损坏".to_string())?;
    let requested_output_mode = match output_mode {
        sdk::LoggingOutputMode::File => LoggingOutputMode::File,
        sdk::LoggingOutputMode::Stderr => LoggingOutputMode::Stderr,
    };
    let current_output_mode = UnifiedLogger::current().map(UnifiedLogger::output_mode);
    match logging_init_decision(current_output_mode, requested_output_mode)? {
        LoggingInitDecision::AlreadyInitialized => return Ok(()),
        LoggingInitDecision::Initialize => {}
    }
    let settings = logging_settings_from_bootstrap(snapshot, default_logs_dir, output_mode);
    UnifiedLogger::init(settings.clone()).map_err(|error| error.to_string())?;
    logging::set_boot_ts(logging::timestamp_local_rfc3339());
    logging::set_app_version(share::version().to_string());
    log::info!(
        target: crate::LOG_TARGET,
        "logging initialized: filter={} mode={:?} logs_dir={} retention_policy_days={}",
        settings.filter_directive(),
        settings.output_mode(),
        settings.logs_dir().display(),
        settings.retention_days(),
    );
    Ok(())
}

pub async fn build_agent_client(args: AgentArgs) -> Result<AgentClientHandle, SdkError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let workspace = project::wire_production_workspace(cwd.clone())
        .map_err(|error| SdkError::Init(error.to_string()))?
        .into_views();
    let logging_output = args.logging_output;
    let agents_dir = share::config::paths::global_agents_dir();
    let config = config::wire_project_config_with_cli(
        &cwd,
        wire_config_override_store(&agents_dir)?,
        cli_config_input(&args),
    )
    .await
    .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    let gateways = FeatureGateways::wire_default(configured_policy(&config));
    init_logging(
        &config.reader().committed_snapshot(),
        logging_output,
        &agents_dir.join("logs"),
    )
    .map_err(|error| SdkError::Init(format!("日志初始化失败：{error}")))?;
    let runtime_client =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config, &agents_dir)
            .await?;
    Ok(agent_client_from_runtime(runtime_client))
}

#[cfg(test)]
async fn build_agent_client_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
    agents_dir: &Path,
) -> Result<AgentClientHandle, SdkError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let workspace = project::wire_production_workspace(cwd.clone())
        .map_err(|error| SdkError::Init(error.to_string()))?
        .into_views();
    let logging_output = args.logging_output;
    // Tests construct the config wiring directly via `ConfigAppService` so the
    // global config path is bounded by the test's `agents_dir` rather than
    // `share::config::paths::global_agents_dir()` (which reads process env vars
    // and would race with parallel tests). See #1385 — the production call site
    // remains env-driven.
    let native_store = wire_config_override_store(agents_dir)?;
    let config = config::wire_project_config_with_agents_dir(
        cwd.as_path(),
        agents_dir,
        native_store,
        cli_config_input(&args),
    )
    .await
    .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    init_logging(
        &config.reader().committed_snapshot(),
        logging_output,
        &agents_dir.join("logs"),
    )
    .map_err(|error| SdkError::Init(format!("日志初始化失败：{error}")))?;
    let runtime_client =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config, agents_dir)
            .await?;
    Ok(agent_client_from_runtime(runtime_client))
}

pub async fn configured_user_agent(args: AgentArgs) -> Result<String, SdkError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let agents_dir = share::config::paths::global_agents_dir();
    let config = config::wire_project_config_with_cli(
        &cwd,
        wire_config_override_store(&agents_dir)?,
        cli_config_input(&args),
    )
    .await
    .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    Ok(config
        .reader()
        .committed_snapshot()
        .user_agent()
        .to_string())
}

pub async fn build_agent_bootstrap(args: AgentArgs) -> Result<AgentClientBootstrap, SdkError> {
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let workspace = project::wire_production_workspace(cwd.clone())
        .map_err(|error| SdkError::Init(error.to_string()))?
        .into_views();
    let logging_output = args.logging_output;
    let agents_dir = share::config::paths::global_agents_dir();
    let config = config::wire_project_config_with_cli(
        &cwd,
        wire_config_override_store(&agents_dir)?,
        cli_config_input(&args),
    )
    .await
    .map_err(|error| SdkError::Init(format!("配置初始化失败：{error:?}")))?;
    let gateways = FeatureGateways::wire_default(configured_policy(&config));
    init_logging(
        &config.reader().committed_snapshot(),
        logging_output,
        &agents_dir.join("logs"),
    )
    .map_err(|error| SdkError::Init(format!("日志初始化失败：{error}")))?;
    let user_agent = config
        .reader()
        .committed_snapshot()
        .user_agent()
        .to_string();
    let config_view = runtime::config_snapshot_to_sdk(&config.reader().committed_snapshot());
    let runtime_client =
        crate::runtime::from_args_with_gateways(args, gateways, workspace, config, &agents_dir)
            .await?;
    let launch = runtime_client.tui_launch_context();
    let command_wiring = crate::tools::wire_commands()
        .map_err(|error| SdkError::Init(format!("命令目录初始化失败：{error}")))?;
    let thinking = launch.binding.requested_reasoning != provider::ReasoningLevel::Off;
    let connect: Arc<dyn sdk::ConnectClient> = wire_connect(&agents_dir);
    let client = agent_client_from_runtime(runtime_client);
    let cwd = launch.workspace_root.clone();

    Ok(AgentClientBootstrap {
        client,
        connect,
        session_id: launch.session_id,
        startup_resume: launch.startup_resume,
        cwd,
        model_display: launch.model_display,
        allow_all: launch.allow_all,
        context_size: launch.context_size,
        thinking,
        config_view,
        memory_config: launch.memory_config,
        skill_snapshot: launch.skill_snapshot,
        command_catalog: command_wiring.catalog(),
        command_router: command_wiring.router(),
        user_agent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::ProviderError;
    use runtime::{ProviderBinding, ProviderBuildSpec, ProviderFactory};
    use share::config::Config;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn logging_init_decision_initializes_when_no_logger_exists() {
        assert_eq!(
            logging_init_decision(None, LoggingOutputMode::File).unwrap(),
            LoggingInitDecision::Initialize
        );
    }

    #[test]
    fn logging_init_decision_is_idempotent_for_same_output_mode() {
        assert_eq!(
            logging_init_decision(Some(LoggingOutputMode::Stderr), LoggingOutputMode::Stderr)
                .unwrap(),
            LoggingInitDecision::AlreadyInitialized
        );
    }

    #[test]
    fn logging_init_decision_rejects_conflicting_output_mode() {
        let error = logging_init_decision(Some(LoggingOutputMode::File), LoggingOutputMode::Stderr)
            .unwrap_err();
        assert!(error.contains("already initialized"));
        assert!(error.contains("File"));
        assert!(error.contains("Stderr"));
    }

    #[derive(Default)]
    struct CountingProviderFactory {
        build_calls: AtomicUsize,
    }

    impl ProviderFactory for CountingProviderFactory {
        fn build(&self, spec: ProviderBuildSpec) -> Result<ProviderBinding, ProviderError> {
            self.build_calls.fetch_add(1, Ordering::SeqCst);
            crate::provider::provider_factory().build(spec)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_agent_client_with_gateways_consumes_injected_provider() {
        let temp = tempfile::tempdir().expect("create temp root");
        let root = temp.path().join("root");
        let agents_dir = temp.path().join("agents");
        std::fs::create_dir_all(&root).expect("create project root");
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
        std::fs::write(agents_dir.join("mcp.json"), r#"{"mcpServers":{}}"#)
            .expect("write MCP config");

        let provider = Arc::new(CountingProviderFactory::default());
        let gateways = FeatureGateways::new(provider.clone(), Arc::new(policy::AllowAllPolicy));
        let args = AgentArgs {
            cwd: Some(root),
            api_key: Some("test-api-key".to_string()),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
            model: Some("local/test-model".to_string()),
            context_size: 8192,
            ..Default::default()
        };

        let result = build_agent_client_with_gateways(args, gateways, &agents_dir).await;

        result.expect("build client with injected gateways");
        assert_eq!(provider.build_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn typed_bootstrap_output_mode_constructs_logging_settings() {
        let snapshot = ConfigSnapshot::new(Config::default());

        let file = logging_settings_from_bootstrap(
            &snapshot,
            Path::new("/fallback/logs"),
            sdk::LoggingOutputMode::File,
        );
        let stderr = logging_settings_from_bootstrap(
            &snapshot,
            Path::new("/fallback/logs"),
            sdk::LoggingOutputMode::Stderr,
        );

        assert_eq!(file.output_mode(), LoggingOutputMode::File);
        assert_eq!(stderr.output_mode(), LoggingOutputMode::Stderr);
    }

    #[test]
    fn snapshot_mapping_preserves_all_logging_settings() {
        let mut config = Config::default();
        config.logging.level = "aemeath:tui=debug,aemeath:agent:runtime=trace".to_string();
        config.logging.max_bytes = 42;
        config.logging.max_backups = 3;
        config.logging.retention_days = 14;
        config.logging.logs_dir = Some("custom/logs".to_string());
        let settings = logging_settings_from_snapshot(
            &ConfigSnapshot::new(config),
            Path::new("/fallback/logs"),
            LoggingOutputMode::Stderr,
        );

        assert_eq!(settings.logs_dir(), PathBuf::from("custom/logs"));
        assert_eq!(settings.max_bytes(), 42);
        assert_eq!(settings.max_backups(), 3);
        assert_eq!(settings.retention_days(), 14);
        assert_eq!(settings.output_mode(), LoggingOutputMode::Stderr);
    }

    #[test]
    fn snapshot_mapping_uses_default_logs_dir_when_config_is_absent() {
        let settings = logging_settings_from_snapshot(
            &ConfigSnapshot::new(Config::default()),
            Path::new("/fallback/logs"),
            LoggingOutputMode::File,
        );
        assert_eq!(settings.logs_dir(), PathBuf::from("/fallback/logs"));
    }
}
