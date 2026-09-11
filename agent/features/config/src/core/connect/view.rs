//! Connect 向导的无密钥 View。
//!
//! ## 设计约束
//!
//! - `ConnectDraftView` **绝不**包含 API key 明文，仅有 `has_api_key`
//!   投影；
//! - `ConnectView` 暴露给 TUI / SDK 用作渲染；所有 view 输入必须来自
//!   service 持有 session 的归一化结果；
//! - `Debug` 输出不得泄漏 API key 明文；服务方应使用 [`crate::connect::ConnectDraft::api_key_redacted`]
//!   之外的展示通道。
//! - `ProbeStatusView` 显式表达 Probe 状态（未跑 / 成功 / 失败）；失败时
//!   携带稳定类别 [`crate::ports::ProviderProbeErrorKind`]。

use crate::catalog::{DriverId, ProviderSource};

use super::error::ConnectError;
use super::outcome::ConnectOutcome;
use super::states::{
    ConnectOrigin, ConnectRevision, ConnectSessionId, ConnectStage, DriverIdOrString,
    ExistingCredentialStatus, ExistingProviderSnapshot,
};
/// Probe 调用结果。`Success { latency }` / `Failed { kind, message }` 中
/// `kind` 镜像 [`crate::ports::ProviderProbeErrorKind`]。
#[derive(Debug, Clone)]
pub enum ProbeStatusView {
    NotRun,
    Running,
    Success {
        latency_ms: u64,
    },
    Failed {
        kind: crate::ports::ProviderProbeErrorKind,
        message: String,
    },
}

/// 模型草稿的投影。无凭证。
#[derive(Debug, Clone, Default)]
pub struct ModelDraftView {
    pub model_id: String,
    pub context_window: Option<usize>,
    pub max_tokens: Option<u32>,
}

/// Connect draft 的无密钥投影。
#[derive(Debug, Clone, Default)]
pub struct ConnectDraftView {
    pub source: Option<ProviderSource>,
    pub driver: Option<DriverId>,
    pub base_url: Option<String>,
    /// 是否有非空 API key。**NEVER** 暴露明文。
    pub has_api_key: bool,
    pub provider_user_agent: Option<String>,
    pub model: Option<ModelDraftView>,
    pub set_global_default: bool,
}

impl ConnectDraftView {
    pub fn source(&self) -> Option<ProviderSource> {
        self.source
    }

    pub fn driver(&self) -> Option<DriverId> {
        self.driver
    }

    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    pub fn provider_user_agent(&self) -> Option<&str> {
        self.provider_user_agent.as_deref()
    }
}

/// Connect 向导单次响应的视图。
///
/// Service 把 session 的内部状态投影为该结构返回给调用方。`Debug` 实现经过
/// 审计：不含 API key、敏感凭证。
#[derive(Debug, Clone)]
pub struct ConnectView {
    pub session_id: ConnectSessionId,
    pub revision: ConnectRevision,
    pub stage: ConnectStage,
    pub origin: ConnectOrigin,
    pub draft: ConnectDraftView,
    pub existing_provider: Option<ExistingProviderSummary>,
    pub available_actions: Vec<AvailableAction>,
    pub probe_status: Option<ProbeStatusView>,
    pub last_error: Option<ConnectError>,
    pub terminal: Option<ConnectOutcome>,
}

/// 现有 Provider 摘要（ConfirmOverwrite 阶段显示）。
#[derive(Debug, Clone)]
pub struct ExistingProviderSummary {
    pub source: String,
    pub driver: Option<String>,
    pub base_url: String,
    pub api_key_status: ExistingCredentialStatus,
    pub model_id: Option<String>,
}

impl From<&ExistingProviderSnapshot> for ExistingProviderSummary {
    fn from(value: &ExistingProviderSnapshot) -> Self {
        Self {
            source: value.source_key.clone(),
            driver: value
                .driver
                .as_ref()
                .map(DriverIdOrString::as_str)
                .map(str::to_string),
            base_url: value.base_url.clone(),
            api_key_status: value.api_key_status,
            model_id: value.model_id.clone(),
        }
    }
}

/// 当前 stage 推荐的可用动作。用于 TUI 按键映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AvailableAction {
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

impl AvailableAction {
    /// 当前 stage 下可用的动作列表。`terminal` 时为空。
    pub fn for_stage(stage: ConnectStage, probe_status: Option<&ProbeStatusView>) -> Vec<Self> {
        match stage {
            ConnectStage::SelectProvider => vec![Self::SelectProvider, Self::Cancel],
            ConnectStage::ConfirmOverwrite => {
                vec![Self::ConfirmOverwrite, Self::RejectOverwrite, Self::Cancel]
            }
            ConnectStage::EditEndpoint => vec![Self::SetEndpoint, Self::Cancel],
            ConnectStage::EditCredential => vec![Self::SetCredential, Self::Cancel],
            ConnectStage::EditUserAgent => vec![Self::SetProviderUserAgent, Self::Cancel],
            ConnectStage::SelectModel => vec![
                Self::SelectRecommendedModel,
                Self::EnterCustomModel,
                Self::Cancel,
            ],
            ConnectStage::EditCustomModel => vec![Self::SetCustomModel, Self::Cancel],
            ConnectStage::ChooseGlobalDefault => vec![Self::SetGlobalDefault, Self::Cancel],
            ConnectStage::ChooseProbe => vec![Self::SkipProbe, Self::BeginProbe, Self::Cancel],
            ConnectStage::Probing => match probe_status {
                Some(ProbeStatusView::Failed { .. }) => vec![
                    Self::ContinueAfterProbe,
                    Self::EditAfterProbeFailure,
                    Self::Cancel,
                ],
                _ => vec![Self::ContinueAfterProbe, Self::Cancel],
            },
            ConnectStage::Review => vec![Self::ConfirmSave, Self::Cancel],
            ConnectStage::Saving => vec![Self::RetrySave, Self::Cancel],
            ConnectStage::Completed | ConnectStage::Cancelled => Vec::new(),
        }
    }
}

impl ConnectView {
    pub fn existing_provider(&self) -> Option<&ExistingProviderSummary> {
        self.existing_provider.as_ref()
    }

    pub fn probe_status(&self) -> Option<&ProbeStatusView> {
        self.probe_status.as_ref()
    }

    pub fn last_error(&self) -> Option<&ConnectError> {
        self.last_error.as_ref()
    }

    pub fn terminal(&self) -> Option<&ConnectOutcome> {
        self.terminal.as_ref()
    }
}
