//! Connect application service —— 服务端持有 Connect session 状态机。
//!
//! ## 设计原则
//!
//! - 服务端是**唯一**的状态拥有者；客户端只见 [`ConnectView`] 与 typed
//!   [`ConnectError`]；
//! - 每个 session 至多产生一次业务终态（Completed / Cancelled）；
//! - 每个命令携带 session id 与预期 revision；过期返回 `StaleRevision`，
//!   非法 stage 返回 `InvalidTransition`；两者都**无副作用**；
//! - draft 仅在 service 内存里持有；View 永远不暴露 API key 明文；
//! - Probe / Commit 都是注入的端口；本服务**不**直接访问 fs / env / 网络；
//! - `SetCustomModel` 校验；catalog 推荐为空时**禁止**构造假推荐；
//!   用户走 `EnterCustomModel` 直接编辑。

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::catalog::{ProviderCatalogEntry, ProviderSource};
use crate::connect::command::expected_stages;
use crate::connect::commit::{ConnectCommitError, ConnectCommitPort, ConnectCommitRequest};
use crate::connect::draft::ConnectDraft;
use crate::connect::error::{command_name as command_name_fn, ConnectError};
use crate::connect::outcome::ConnectOutcome;
use crate::connect::states::{
    ConnectOrigin, ConnectRevision, ConnectSessionId, ConnectStage, ExistingProviderSnapshot,
};
use crate::connect::view::{
    AvailableAction, ConnectDraftView, ConnectView, ModelDraftView, ProbeStatusView,
};
use crate::connect::ModelDraft;
use crate::ports::{ProviderProbePort, ProviderProbeRequest, SystemInformation};
use crate::user_agent::{resolve_provider_user_agent, ProviderUserAgentInputs};

/// Probe 调用注入的合法超时上限。该值是 Connect 服务的策略常量，不在
/// 客户端控制范围内，避免各入口漂移。
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// single shot Session。承载 draft、当前 stage、revision 与 probe 状态。
struct ConnectSession {
    session_id: ConnectSessionId,
    revision: ConnectRevision,
    origin: ConnectOrigin,
    expected_global_revision: crate::GlobalConfigRevision,
    stage: ConnectStage,
    draft: ConnectDraft,
    probe_status: Option<ProbeStatusView>,
    existing_provider: Option<ExistingProviderSnapshot>,
    /// 最近一次命令错误的投影。终态置 `None`，UI 据此判定是否显示提示。
    last_error: Option<ConnectError>,
    /// 终态 outcome，发布后 session 锁定后续命令。
    outcome: Option<ConnectOutcome>,
}

impl ConnectSession {
    fn new(
        session_id: ConnectSessionId,
        origin: ConnectOrigin,
        expected_global_revision: crate::GlobalConfigRevision,
        existing_provider: Option<ExistingProviderSnapshot>,
    ) -> Self {
        let stage = if existing_provider.is_some() {
            ConnectStage::ConfirmOverwrite
        } else {
            ConnectStage::SelectProvider
        };
        let mut draft = ConnectDraft::empty();
        if let Some(provider) = existing_provider.as_ref() {
            if let Some(catalog_source) = provider.catalog_source() {
                draft.source = Some(catalog_source);
            }
            if let Some(driver) = provider.driver.as_ref().and_then(|d| d.as_known()) {
                draft.driver = Some(*driver);
            }
            if !provider.base_url.is_empty() {
                draft.base_url = Some(provider.base_url.clone());
            }
            // 覆盖确认路径：保留 has_api_key 投影；新凭证在 SetCredential 时由用户重写。
            // 注意：进入 ConfirmOverwrite 阶段时，session 还没"确认覆盖"，
            // 因此 credential 仍是 NotSet；待 ConfirmOverwrite 命令调用后
            // 才切到 PreservedFromExisting。
            draft.credential = crate::connect::draft::CredentialState::NotSet;
        }
        Self {
            session_id,
            revision: ConnectRevision::initial(),
            origin,
            expected_global_revision,
            stage,
            draft,
            probe_status: None,
            existing_provider,
            last_error: None,
            outcome: None,
        }
    }
}

/// ConnectAppService 是状态机的服务拥有者。
pub struct ConnectAppService {
    pub(crate) catalog: &'static [ProviderCatalogEntry],
    pub(crate) probe: Arc<dyn ProviderProbePort>,
    /// 测试 / 真实 adapter 可能未注入；service 在 Save 路径据此返回
    /// `PersistUnavailable`，确保 UI 立刻失效而非偷偷失败。
    commit: Option<Arc<dyn ConnectCommitPort>>,
    sessions: Mutex<std::collections::HashMap<ConnectSessionId, Arc<Mutex<ConnectSession>>>>,
    pub(crate) system: SystemInformation,
    pub(crate) version: &'static str,
}

/// `ConnectAppService` 的 builder。测试 / production 都需要相同入口。
pub struct ConnectAppServiceBuilder {
    catalog: Option<&'static [ProviderCatalogEntry]>,
    probe: Option<Arc<dyn ProviderProbePort>>,
    commit_state: CommitSlot,
    system: Option<SystemInformation>,
    version: Option<&'static str>,
}

/// `Option<Option<...>>` 的清晰表达：None ↔ 显式不要；Some(None) ↔
/// 待注入占位；Some(Some(c)) ↔ 已注入。
#[derive(Default)]
enum CommitSlot {
    #[default]
    Absent,
    Present(Option<Arc<dyn ConnectCommitPort>>),
}

impl ConnectAppServiceBuilder {
    pub fn with_catalog(mut self, catalog: &'static [ProviderCatalogEntry]) -> Self {
        self.catalog = Some(catalog);
        self
    }

    pub fn with_probe(mut self, probe: Arc<dyn ProviderProbePort>) -> Self {
        self.probe = Some(probe);
        self
    }

    pub fn with_commit(mut self, commit: Arc<dyn ConnectCommitPort>) -> Self {
        self.commit_state = CommitSlot::Present(Some(commit));
        self
    }

    /// 等价于 `with_commit(None)` 的显式删除样式，避免 `Option` 嵌套歧义。
    pub fn without_commit(mut self) -> Self {
        self.commit_state = CommitSlot::Present(None);
        self
    }

    pub fn with_system(mut self, system: SystemInformation) -> Self {
        self.system = Some(system);
        self
    }

    pub fn with_version(mut self, version: &'static str) -> Self {
        self.version = Some(version);
        self
    }

    /// 构建 [`ConnectAppService`]。`catalog` 与 `probe` 必须提供；其他可选。
    pub fn build(self) -> ConnectAppService {
        let commit = match self.commit_state {
            CommitSlot::Absent => None,
            CommitSlot::Present(inner) => inner,
        };
        ConnectAppService {
            catalog: self
                .catalog
                .expect("ConnectAppService 必须注入 Provider Catalog"),
            probe: self
                .probe
                .expect("ConnectAppService 必须注入 ProviderProbePort"),
            commit,
            sessions: Mutex::new(std::collections::HashMap::new()),
            system: self.system.unwrap_or_else(|| SystemInformation {
                os_name: "unknown-os".into(),
                os_version: None,
                arch: "unknown-arch".into(),
            }),
            version: self.version.unwrap_or(env!("CARGO_PKG_VERSION")),
        }
    }
}

impl Default for ConnectAppServiceBuilder {
    fn default() -> Self {
        Self {
            catalog: None,
            probe: None,
            commit_state: CommitSlot::Absent,
            system: None,
            version: None,
        }
    }
}

impl ConnectAppService {
    /// 创建 builder。
    pub fn builder() -> ConnectAppServiceBuilder {
        ConnectAppServiceBuilder::default()
    }

    /// 创建新 session 并返回初始 view。`existing_provider` 决定初始 stage。
    pub async fn start_connect(
        &self,
        origin: ConnectOrigin,
        expected_global_revision: crate::GlobalConfigRevision,
        existing_provider: Option<ExistingProviderSnapshot>,
    ) -> ConnectView {
        let session_id = ConnectSessionId::new();
        let session = ConnectSession::new(
            session_id,
            origin,
            expected_global_revision,
            existing_provider,
        );
        let view = self.project_view(&session);
        self.sessions
            .lock()
            .await
            .insert(session_id, Arc::new(Mutex::new(session)));
        view
    }

    /// 在用户选择 source 前，把同名现有 Provider 的脱敏快照附加到 session。
    pub async fn attach_existing_provider(
        &self,
        session_id: ConnectSessionId,
        expected_revision: ConnectRevision,
        existing_provider: ExistingProviderSnapshot,
    ) -> Result<(), ConnectError> {
        let session = self.sessions.lock().await.get(&session_id).cloned().ok_or(
            ConnectError::InvalidTransition {
                command: "AttachExistingProvider",
                actual: ConnectStage::Cancelled,
            },
        )?;
        let mut session = session.lock().await;
        if session.stage != ConnectStage::SelectProvider || session.outcome.is_some() {
            return Err(ConnectError::InvalidTransition {
                command: "AttachExistingProvider",
                actual: session.stage,
            });
        }
        if session.revision != expected_revision {
            return Err(ConnectError::StaleRevision {
                actual: session.revision,
                provided: expected_revision,
            });
        }
        session.existing_provider = Some(existing_provider);
        Ok(())
    }

    /// 取得当前 session 的最新 view。
    pub async fn view(&self, session_id: ConnectSessionId) -> Option<ConnectView> {
        let session = self.sessions.lock().await.get(&session_id).cloned()?;
        let session = session.lock().await;
        Some(self.project_view(&session))
    }

    /// 取消 session。
    pub async fn cancel(
        &self,
        session_id: ConnectSessionId,
        expected_revision: ConnectRevision,
    ) -> Result<ConnectView, ConnectError> {
        let session = self.sessions.lock().await.get(&session_id).cloned().ok_or(
            ConnectError::InvalidTransition {
                command: "Cancel",
                actual: ConnectStage::Cancelled,
            },
        )?;
        let mut session = session.lock().await;
        if session.outcome.is_some() {
            return Err(ConnectError::InvalidTransition {
                command: "Cancel",
                actual: session.stage,
            });
        }
        if session.revision != expected_revision {
            return Err(ConnectError::StaleRevision {
                actual: session.revision,
                provided: expected_revision,
            });
        }
        session.stage = ConnectStage::Cancelled;
        session.outcome = Some(ConnectOutcome::Cancelled);
        session.revision = bump_revision(session.revision);
        Ok(self.project_view(&session))
    }

    /// 核心推进入口。校验 → 同步 handler → 异步副作用（BeginProbe /
    /// ConfirmSave）→ 以 operation revision 合并结果 / 投影。
    pub async fn apply(
        &self,
        session_id: ConnectSessionId,
        expected_revision: ConnectRevision,
        command: crate::connect::ConnectCommand,
    ) -> Result<ConnectView, ConnectError> {
        let session = self.sessions.lock().await.get(&session_id).cloned().ok_or(
            ConnectError::InvalidTransition {
                command: command_name_fn(&command),
                actual: ConnectStage::Cancelled,
            },
        )?;
        let mut session_guard = session.lock().await;

        if session_guard.outcome.is_some() {
            return Err(ConnectError::InvalidTransition {
                command: command_name_fn(&command),
                actual: session_guard.stage,
            });
        }
        if session_guard.revision != expected_revision {
            return Err(ConnectError::StaleRevision {
                actual: session_guard.revision,
                provided: expected_revision,
            });
        }
        let expected = expected_stages(&command);
        if !expected.contains(&session_guard.stage) {
            return Err(ConnectError::InvalidTransition {
                command: command_name_fn(&command),
                actual: session_guard.stage,
            });
        }

        let (err, sync_outcome) = self.handle_sync(&mut session_guard, &command);
        if let Some(error) = err.clone() {
            session_guard.last_error = Some(error.clone());
            return Err(error);
        }
        let _ = sync_outcome;

        let operation = match command {
            crate::connect::ConnectCommand::BeginProbe => {
                session_guard.stage = ConnectStage::Probing;
                session_guard.probe_status = Some(ProbeStatusView::Running);
                session_guard.last_error = None;
                session_guard.revision = bump_revision(session_guard.revision);
                let revision = session_guard.revision;
                let request = self.prepare_probe_request(&session_guard)?;
                Some((revision, AsyncOperation::Probe(request)))
            }
            crate::connect::ConnectCommand::ConfirmSave => {
                session_guard.stage = ConnectStage::Saving;
                session_guard.last_error = None;
                session_guard.revision = bump_revision(session_guard.revision);
                let revision = session_guard.revision;
                let request = self.prepare_commit_request(&session_guard);
                Some((revision, AsyncOperation::Commit(request)))
            }
            _ => None,
        };

        let Some((operation_revision, operation)) = operation else {
            session_guard.revision = bump_revision(session_guard.revision);
            return Ok(self.project_view(&session_guard));
        };
        drop(session_guard);

        let async_result = self.run_async_operation(operation).await;
        let mut session_guard = session.lock().await;
        if session_guard.revision != operation_revision || session_guard.outcome.is_some() {
            return Ok(self.project_view(&session_guard));
        }
        self.apply_async_outcome(&mut session_guard, async_result);
        session_guard.revision = bump_revision(session_guard.revision);
        Ok(self.project_view(&session_guard))
    }

    fn prepare_probe_request(
        &self,
        session: &ConnectSession,
    ) -> Result<ProviderProbeRequest, ConnectError> {
        let driver = session
            .draft
            .driver
            .ok_or(ConnectError::InvalidTransition {
                command: "BeginProbe",
                actual: session.stage,
            })?;
        let base_url = session
            .draft
            .base_url
            .clone()
            .ok_or(ConnectError::InvalidTransition {
                command: "BeginProbe",
                actual: session.stage,
            })?;
        let model = session
            .draft
            .model
            .clone()
            .ok_or(ConnectError::InvalidTransition {
                command: "BeginProbe",
                actual: session.stage,
            })?;
        Ok(ProviderProbeRequest {
            driver,
            base_url,
            credential: session.draft.api_key_plaintext().map(str::to_string),
            model_id: model.model_id,
            context_window: model.context_window,
            max_tokens: model.max_tokens,
            final_user_agent: self.resolve_user_agent(&session.draft),
            timeout: PROBE_TIMEOUT,
        })
    }

    fn prepare_commit_request(&self, session: &ConnectSession) -> ConnectCommitRequest {
        ConnectCommitRequest {
            session_id: session.session_id,
            origin: session.origin,
            expected_global_revision: session.expected_global_revision.clone(),
            draft: session.draft.clone(),
        }
    }

    async fn run_async_operation(&self, operation: AsyncOperation) -> AsyncOutcome {
        match operation {
            AsyncOperation::Probe(request) => self.run_probe(request).await,
            AsyncOperation::Commit(request) => self.run_commit(request).await,
        }
    }

    fn apply_async_outcome(&self, session: &mut ConnectSession, outcome: AsyncOutcome) {
        match outcome {
            AsyncOutcome::ProbeSuccess { latency_ms } => {
                session.probe_status = Some(ProbeStatusView::Success { latency_ms });
                session.stage = ConnectStage::Review;
                session.last_error = None;
            }
            AsyncOutcome::ProbeFailed { kind, message } => {
                session.probe_status = Some(ProbeStatusView::Failed { kind, message });
                session.stage = ConnectStage::Probing;
                session.last_error = Some(ConnectError::ProbeFailed {
                    kind,
                    message: "探测失败".to_string(),
                });
            }
            AsyncOutcome::CommitSuccess { applied_revision } => {
                session.draft.credential = crate::connect::draft::CredentialState::NotSet;
                session.stage = ConnectStage::Completed;
                session.outcome = Some(ConnectOutcome::Completed { applied_revision });
                session.last_error = None;
            }
            AsyncOutcome::CommitFailed(error) => {
                session.stage = ConnectStage::Saving;
                session.last_error = Some(error);
            }
        }
    }

    /// 同步 handler。把 stage 修改 / draft 修改直接写入 session，
    /// 返回 (Option<error>, SyncOutcome)。
    fn handle_sync(
        &self,
        session: &mut ConnectSession,
        command: &crate::connect::ConnectCommand,
    ) -> (Option<ConnectError>, SyncOutcome) {
        use crate::connect::ConnectCommand as Cmd;
        match command {
            Cmd::SelectProvider { source } => self.sync_select_provider(session, *source),
            Cmd::ConfirmOverwrite => self.sync_confirm_overwrite(session),
            Cmd::RejectOverwrite => self.sync_reject_overwrite(session),
            Cmd::SetEndpoint { base_url } => self.sync_set_endpoint(session, base_url),
            Cmd::SetCredential { api_key } => self.sync_set_credential(session, api_key),
            Cmd::SetProviderUserAgent { raw } => {
                self.sync_set_provider_user_agent(session, raw.as_deref())
            }
            Cmd::SelectRecommendedModel { index } => {
                self.sync_select_recommended_model(session, *index)
            }
            Cmd::EnterCustomModel => self.sync_enter_custom_model(session),
            Cmd::SetCustomModel {
                model_id,
                context_window,
                max_tokens,
            } => self.sync_set_custom_model(session, model_id, *context_window, *max_tokens),
            Cmd::SetGlobalDefault { set_as_default } => {
                self.sync_set_global_default(session, *set_as_default)
            }
            Cmd::SkipProbe => self.sync_skip_probe(session),
            Cmd::BeginProbe => (None, SyncOutcome::Proceed), // handled in async
            Cmd::ContinueAfterProbe => self.sync_continue_after_probe(session),
            Cmd::EditAfterProbeFailure => self.sync_edit_after_probe_failure(session),
            Cmd::ConfirmSave => (None, SyncOutcome::Proceed), // handled in async
        }
    }

    fn catalog_entry(&self, source: ProviderSource) -> Option<&'static ProviderCatalogEntry> {
        self.catalog.iter().find(|entry| entry.source == source)
    }

    fn sync_select_provider(
        &self,
        session: &mut ConnectSession,
        source: ProviderSource,
    ) -> (Option<ConnectError>, SyncOutcome) {
        if self.catalog_entry(source).is_none() {
            return (
                Some(ConnectError::CatalogUnavailable {
                    reason: format!("未知 source: {}", source.as_str()),
                }),
                SyncOutcome::Proceed,
            );
        }
        let entry = self.catalog_entry(source).expect("checked above");
        session.draft.source = Some(source);
        session.draft.driver = Some(entry.driver);
        if session
            .existing_provider
            .as_ref()
            .is_some_and(|provider| provider.source_key == source.as_str())
        {
            session.stage = ConnectStage::ConfirmOverwrite;
            return (None, SyncOutcome::Proceed);
        }
        if session.draft.base_url.is_none() {
            if let Some(endpoint) = entry.default_endpoint {
                session.draft.base_url = Some(endpoint.url.to_string());
            }
        }
        session.stage = ConnectStage::EditEndpoint;
        (None, SyncOutcome::Proceed)
    }

    fn sync_confirm_overwrite(
        &self,
        session: &mut ConnectSession,
    ) -> (Option<ConnectError>, SyncOutcome) {
        session.stage = ConnectStage::EditEndpoint;
        // ConfirmOverwrite 之后，credential 标为 PreservedFromExisting；
        // 若用户再调 SetCredential，状态会切到 UserSet/NotSet。
        if let Some(provider) = session.existing_provider.as_ref() {
            if matches!(
                provider.api_key_status,
                super::states::ExistingCredentialStatus::Present
            ) {
                session.draft.preserve_existing_credential();
            } else {
                session.draft.set_user_credential(String::new());
            }
        }
        (None, SyncOutcome::Proceed)
    }

    fn sync_reject_overwrite(
        &self,
        session: &mut ConnectSession,
    ) -> (Option<ConnectError>, SyncOutcome) {
        session.stage = ConnectStage::SelectProvider;
        session.draft.source = None;
        session.draft.driver = None;
        session.draft.base_url = None;
        session.draft.credential = crate::connect::draft::CredentialState::NotSet;
        session.draft.provider_user_agent = None;
        (None, SyncOutcome::Proceed)
    }

    fn sync_set_endpoint(
        &self,
        session: &mut ConnectSession,
        base_url: &str,
    ) -> (Option<ConnectError>, SyncOutcome) {
        match ConnectDraft::normalize_base_url(base_url) {
            Ok(value) => {
                session.draft.base_url = Some(value);
                session.stage = ConnectStage::EditCredential;
                (None, SyncOutcome::Proceed)
            }
            Err(err) => (
                Some(ConnectError::Validation {
                    field: "endpoint",
                    reason: err.message().to_string(),
                }),
                SyncOutcome::Proceed,
            ),
        }
    }

    fn sync_set_credential(
        &self,
        session: &mut ConnectSession,
        api_key: &str,
    ) -> (Option<ConnectError>, SyncOutcome) {
        session.draft.set_user_credential(api_key.to_string());
        session.stage = ConnectStage::EditUserAgent;
        (None, SyncOutcome::Proceed)
    }

    fn sync_set_provider_user_agent(
        &self,
        session: &mut ConnectSession,
        raw: Option<&str>,
    ) -> (Option<ConnectError>, SyncOutcome) {
        match ConnectDraft::validate_provider_user_agent(raw) {
            Ok(value) => {
                session.draft.provider_user_agent = value;
                session.stage = ConnectStage::SelectModel;
                (None, SyncOutcome::Proceed)
            }
            Err(err) => (
                Some(ConnectError::Validation {
                    field: "provider_user_agent",
                    reason: err.message().to_string(),
                }),
                SyncOutcome::Proceed,
            ),
        }
    }

    fn sync_select_recommended_model(
        &self,
        session: &mut ConnectSession,
        index: usize,
    ) -> (Option<ConnectError>, SyncOutcome) {
        let source = match session.draft.source {
            Some(s) => s,
            None => {
                return (
                    Some(ConnectError::InvalidTransition {
                        command: "SelectRecommendedModel",
                        actual: session.stage,
                    }),
                    SyncOutcome::Proceed,
                );
            }
        };
        let entry = match self.catalog_entry(source) {
            Some(e) => e,
            None => {
                return (
                    Some(ConnectError::CatalogUnavailable {
                        reason: format!("source {} 已不在 Catalog 中", source.as_str()),
                    }),
                    SyncOutcome::Proceed,
                );
            }
        };
        if entry.recommended_models.is_empty() {
            return (
                Some(ConnectError::CatalogUnavailable {
                    reason: format!(
                        "{} 当前没有核验过的推荐模型，请直接进入 EditCustomModel",
                        source.as_str()
                    ),
                }),
                SyncOutcome::Proceed,
            );
        }
        let model = match entry.recommended_models.get(index) {
            Some(m) => m,
            None => {
                return (
                    Some(ConnectError::Validation {
                        field: "recommended_model_index",
                        reason: format!(
                            "索引 {index} 超出 Catalog 推荐列表（长度 {}）",
                            entry.recommended_models.len()
                        ),
                    }),
                    SyncOutcome::Proceed,
                );
            }
        };
        session.draft.model = Some(ModelDraft {
            model_id: model.model_id.to_string(),
            context_window: model.context_window,
            max_tokens: model.max_tokens,
        });
        session.stage = ConnectStage::ChooseGlobalDefault;
        (None, SyncOutcome::Proceed)
    }

    fn sync_enter_custom_model(
        &self,
        session: &mut ConnectSession,
    ) -> (Option<ConnectError>, SyncOutcome) {
        session.stage = ConnectStage::EditCustomModel;
        (None, SyncOutcome::Proceed)
    }

    fn sync_set_custom_model(
        &self,
        session: &mut ConnectSession,
        model_id: &str,
        context_window: usize,
        max_tokens: u32,
    ) -> (Option<ConnectError>, SyncOutcome) {
        let draft = ModelDraft {
            model_id: model_id.to_string(),
            context_window,
            max_tokens,
        };
        if let Err(err) = draft.validate() {
            return (
                Some(ConnectError::Validation {
                    field: "model",
                    reason: err.message().to_string(),
                }),
                SyncOutcome::Proceed,
            );
        }
        session.draft.model = Some(draft);
        session.stage = ConnectStage::ChooseGlobalDefault;
        (None, SyncOutcome::Proceed)
    }

    fn sync_set_global_default(
        &self,
        session: &mut ConnectSession,
        set_as_default: bool,
    ) -> (Option<ConnectError>, SyncOutcome) {
        session.draft.set_global_default = set_as_default;
        session.stage = ConnectStage::ChooseProbe;
        (None, SyncOutcome::Proceed)
    }

    fn sync_skip_probe(&self, session: &mut ConnectSession) -> (Option<ConnectError>, SyncOutcome) {
        session.probe_status = None;
        session.stage = ConnectStage::Review;
        (None, SyncOutcome::Proceed)
    }

    fn sync_continue_after_probe(
        &self,
        session: &mut ConnectSession,
    ) -> (Option<ConnectError>, SyncOutcome) {
        match session.probe_status.as_ref() {
            Some(ProbeStatusView::Failed { .. } | ProbeStatusView::Success { .. }) => {
                session.stage = ConnectStage::Review;
                (None, SyncOutcome::Proceed)
            }
            _ => (
                Some(ConnectError::InvalidTransition {
                    command: "ContinueAfterProbe",
                    actual: session.stage,
                }),
                SyncOutcome::Proceed,
            ),
        }
    }

    fn sync_edit_after_probe_failure(
        &self,
        session: &mut ConnectSession,
    ) -> (Option<ConnectError>, SyncOutcome) {
        match session.probe_status.as_ref() {
            Some(ProbeStatusView::Failed { .. }) => {
                session.stage = ConnectStage::EditEndpoint;
                (None, SyncOutcome::Proceed)
            }
            _ => (
                Some(ConnectError::InvalidTransition {
                    command: "EditAfterProbeFailure",
                    actual: session.stage,
                }),
                SyncOutcome::Proceed,
            ),
        }
    }

    async fn run_probe(&self, request: ProviderProbeRequest) -> AsyncOutcome {
        match self.probe.probe(request).await {
            Ok(result) => AsyncOutcome::ProbeSuccess {
                latency_ms: result.latency.as_millis() as u64,
            },
            Err(error) => AsyncOutcome::ProbeFailed {
                kind: error.kind,
                message: error.message,
            },
        }
    }

    async fn run_commit(&self, request: ConnectCommitRequest) -> AsyncOutcome {
        let Some(commit) = self.commit.as_ref() else {
            return AsyncOutcome::CommitFailed(ConnectError::PersistUnavailable);
        };
        match commit.commit(request).await {
            Ok(receipt) => AsyncOutcome::CommitSuccess {
                applied_revision: receipt.applied_revision,
            },
            Err(ConnectCommitError::PersistConflict { expected }) => {
                AsyncOutcome::CommitFailed(ConnectError::PersistConflict { expected })
            }
            Err(ConnectCommitError::PersistFailed { kind, message }) => {
                AsyncOutcome::CommitFailed(ConnectError::PersistFailed { kind, message })
            }
            Err(ConnectCommitError::PersistUnavailable) => {
                AsyncOutcome::CommitFailed(ConnectError::PersistUnavailable)
            }
        }
    }

    fn resolve_user_agent(&self, draft: &ConnectDraft) -> String {
        let inputs = ProviderUserAgentInputs {
            provider_user_agent: draft.provider_user_agent.as_deref(),
            catalog_official_sdk_user_agent: None,
            global_user_agent: None,
            system: self.system.clone(),
            version: self.version,
        };
        resolve_provider_user_agent(inputs)
            .to_str()
            .map(str::to_string)
            .unwrap_or_else(|_| "Aemeath/0.0.0 cli unknown-os/unknown-arch".to_string())
    }

    fn project_view(&self, session: &ConnectSession) -> ConnectView {
        let stage = session.stage;
        let probe_status = session.probe_status.clone();
        let available_actions = AvailableAction::for_stage(stage, probe_status.as_ref());
        ConnectView {
            session_id: session.session_id,
            revision: session.revision,
            stage,
            origin: session.origin,
            draft: project_draft(&session.draft),
            existing_provider: session.existing_provider.as_ref().map(Into::into),
            available_actions,
            probe_status,
            last_error: session.last_error.clone(),
            terminal: session.outcome.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SyncOutcome {
    Proceed,
}

enum AsyncOperation {
    Probe(ProviderProbeRequest),
    Commit(ConnectCommitRequest),
}

enum AsyncOutcome {
    ProbeSuccess {
        latency_ms: u64,
    },
    ProbeFailed {
        kind: crate::ports::ProviderProbeErrorKind,
        message: String,
    },
    CommitSuccess {
        applied_revision: u64,
    },
    CommitFailed(ConnectError),
}

fn bump_revision(revision: ConnectRevision) -> ConnectRevision {
    revision.bump()
}

fn project_draft(draft: &ConnectDraft) -> ConnectDraftView {
    ConnectDraftView {
        source: draft.source,
        driver: draft.driver,
        base_url: draft.base_url.clone(),
        has_api_key: draft.has_api_key(),
        provider_user_agent: draft.provider_user_agent.clone(),
        model: draft.model.as_ref().map(|model| ModelDraftView {
            model_id: model.model_id.clone(),
            context_window: Some(model.context_window),
            max_tokens: Some(model.max_tokens),
        }),
        set_global_default: draft.set_global_default,
    }
}
