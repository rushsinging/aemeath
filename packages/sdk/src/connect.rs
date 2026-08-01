use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ConnectSessionId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ConnectRevision(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectOrigin {
    ExplicitCommand,
    FirstChatBootstrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectStage {
    SelectProvider,
    ConfirmOverwrite,
    EditEndpoint,
    EditCredential,
    EditUserAgent,
    SelectModel,
    EditCustomModel,
    ChooseGlobalDefault,
    ChooseProbe,
    Probing,
    Review,
    Saving,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectCommand {
    SelectProvider {
        source: String,
    },
    ConfirmOverwrite,
    RejectOverwrite,
    SetEndpoint {
        base_url: String,
    },
    SetCredential {
        api_key: String,
    },
    SetProviderUserAgent {
        raw: Option<String>,
    },
    SelectRecommendedModel {
        index: usize,
    },
    EnterCustomModel,
    SetCustomModel {
        model_id: String,
        context_window: usize,
        max_tokens: u32,
    },
    SetGlobalDefault {
        set_as_default: bool,
    },
    SkipProbe,
    BeginProbe,
    ContinueAfterProbe,
    EditAfterProbeFailure,
    ConfirmSave,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectModelDraftView {
    pub model_id: String,
    pub context_window: Option<usize>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectDraftView {
    pub source: Option<String>,
    pub driver: Option<String>,
    pub base_url: Option<String>,
    pub has_api_key: bool,
    pub provider_user_agent: Option<String>,
    pub model: Option<ConnectModelDraftView>,
    pub set_global_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectExistingProviderView {
    pub source: String,
    pub driver: Option<String>,
    pub base_url: String,
    pub has_api_key: bool,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectAvailableAction {
    SelectProvider,
    ConfirmOverwrite,
    RejectOverwrite,
    SetEndpoint,
    SetCredential,
    SetProviderUserAgent,
    SelectRecommendedModel,
    EnterCustomModel,
    SetCustomModel,
    SetGlobalDefault,
    SkipProbe,
    BeginProbe,
    ContinueAfterProbe,
    EditAfterProbeFailure,
    ConfirmSave,
    RetrySave,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectProbeErrorKind {
    Cancelled,
    Timeout,
    Authentication,
    Endpoint,
    Model,
    Protocol,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectProbeStatus {
    NotRun,
    Running,
    Success {
        latency_ms: u64,
    },
    Failed {
        kind: ConnectProbeErrorKind,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectErrorKind {
    InvalidTransition,
    StaleRevision,
    Validation,
    CatalogUnavailable,
    ProbeFailed,
    PersistConflict,
    PersistFailed,
    PersistUnavailable,
    InteractiveSetupRequired,
    BootstrapRollbackRefused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectErrorView {
    pub kind: ConnectErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectOutcome {
    Completed { applied_revision: u64 },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectRecommendedModelOption {
    pub model_id: String,
    pub context_window: usize,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectProviderOption {
    pub source: String,
    pub driver: String,
    pub default_base_url: String,
    pub recommended_models: Vec<ConnectRecommendedModelOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectView {
    pub session_id: ConnectSessionId,
    pub revision: ConnectRevision,
    pub stage: ConnectStage,
    pub origin: ConnectOrigin,
    pub catalog: Vec<ConnectProviderOption>,
    pub draft: ConnectDraftView,
    pub existing_provider: Option<ConnectExistingProviderView>,
    pub available_actions: Vec<ConnectAvailableAction>,
    pub probe_status: Option<ConnectProbeStatus>,
    pub last_error: Option<ConnectErrorView>,
    pub terminal: Option<ConnectOutcome>,
}
