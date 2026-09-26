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

use crate::catalog::{DriverId, ProviderCatalogEntry, ProviderSource};
use crate::connect::command::expected_stages;
use crate::connect::commit::{
    ConnectCommitError, ConnectCommitPort, ConnectCommitRequest, ConnectProviderDirectory,
};
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
use crate::user_agent::{
    assemble_provider_user_agent_inputs, resolve_provider_user_agent_str, ProviderUserAgentRequest,
};

/// Probe 调用注入的合法超时上限。该值是 Connect 服务的策略常量，不在
/// 客户端控制范围内，避免各入口漂移。
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// single shot Session。承载 draft、当前 stage、revision 与 probe 状态。
struct ConnectSession {
    session_id: ConnectSessionId,
    revision: ConnectRevision,
    origin: ConnectOrigin,
    expected_global_revision: crate::global_store::GlobalConfigRevision,
    stage: ConnectStage,
    draft: ConnectDraft,
    probe_status: Option<ProbeStatusView>,
    /// start 时一次性加载的已有 Provider 快照（source key → 脱敏快照）。
    /// SelectProvider 的 ConfirmOverwrite 判断与预填全部走该内存快照，
    /// source 选择路径上 **NEVER** 再做 IO。
    existing_providers: std::collections::HashMap<String, ExistingProviderSnapshot>,
    /// 当前选中 source 对应的快照（ConfirmOverwrite 阶段的 view 投影）。
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
        expected_global_revision: crate::global_store::GlobalConfigRevision,
        existing_providers: std::collections::HashMap<String, ExistingProviderSnapshot>,
    ) -> Self {
        Self {
            session_id,
            revision: ConnectRevision::initial(),
            origin,
            expected_global_revision,
            stage: ConnectStage::SelectProvider,
            draft: ConnectDraft::empty(),
            probe_status: None,
            existing_providers,
            existing_provider: None,
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
    /// start_connect 单点加载 Provider 目录快照的读端口。
    provider_directory: Option<Arc<dyn ConnectProviderDirectory>>,
    sessions: std::sync::Arc<
        Mutex<std::collections::HashMap<ConnectSessionId, Arc<Mutex<ConnectSession>>>>,
    >,
    pub(crate) system: SystemInformation,
    pub(crate) version: &'static str,
    /// 全局配置 `api.user_agent` 的装配期快照。
    ///
    /// Connect probe 与正式 Provider 请求**MUST**来自同一份全局 UA；装配者负责在
    /// 构造 service 时注入当前值。`None` 或空白等同未配置并继续回退。
    pub(crate) global_user_agent: Option<String>,
}

/// `ConnectAppService` 的 builder。测试 / production 都需要相同入口。
pub struct ConnectAppServiceBuilder {
    catalog: Option<&'static [ProviderCatalogEntry]>,
    probe: Option<Arc<dyn ProviderProbePort>>,
    commit_state: CommitSlot,
    provider_directory: Option<Arc<dyn ConnectProviderDirectory>>,
    system: Option<SystemInformation>,
    version: Option<&'static str>,
    global_user_agent: Option<String>,
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

    /// 注入 Provider 目录读端口：start_connect 单点加载已有 Provider 快照。
    pub fn with_provider_directory(
        mut self,
        provider_directory: Arc<dyn ConnectProviderDirectory>,
    ) -> Self {
        self.provider_directory = Some(provider_directory);
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

    /// 注入全局配置 `api.user_agent`；`None` 或空白表示未配置。
    pub fn with_global_user_agent(mut self, global_user_agent: Option<String>) -> Self {
        self.global_user_agent = global_user_agent;
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
            provider_directory: self.provider_directory,
            sessions: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
            system: self.system.unwrap_or_else(|| SystemInformation {
                os_name: "unknown-os".into(),
                os_version: None,
                arch: "unknown-arch".into(),
            }),
            version: self.version.unwrap_or(env!("CARGO_PKG_VERSION")),
            global_user_agent: self.global_user_agent,
        }
    }
}

impl Default for ConnectAppServiceBuilder {
    fn default() -> Self {
        Self {
            catalog: None,
            probe: None,
            commit_state: CommitSlot::Absent,
            provider_directory: None,
            system: None,
            version: None,
            global_user_agent: None,
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
        expected_global_revision: crate::global_store::GlobalConfigRevision,
    ) -> ConnectView {
        // Provider 目录快照在此单点加载；读失败降级为空目录（向导仍可用，
        // 只是不触发 ConfirmOverwrite / 已有值预填），不阻断会话。
        let existing_providers = match self.provider_directory.as_ref() {
            Some(directory) => directory
                .provider_snapshots()
                .await
                .map(|snapshots| {
                    snapshots
                        .into_iter()
                        .map(|snapshot| (snapshot.source_key.clone(), snapshot))
                        .collect()
                })
                .unwrap_or_default(),
            None => std::collections::HashMap::new(),
        };
        let session_id = ConnectSessionId::new();
        let session = ConnectSession::new(
            session_id,
            origin,
            expected_global_revision,
            existing_providers,
        );
        let view = self.project_view(&session);
        self.sessions
            .lock()
            .await
            .insert(session_id, Arc::new(Mutex::new(session)));
        view
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

        // 立即返回 Running 视图（TUI busy 轮询经 refresh_form 观察进度）；
        // 异步结果由后台 task 写回：revision 未变且未终态才落盘，避免覆盖
        // 期间用户命令（Back / Cancel）的状态迁移。
        let sessions = self.sessions.clone();
        let probe = self.probe.clone();
        let commit = self.commit.clone();
        let session_id = session_guard.session_id;
        drop(session_guard);
        tokio::spawn(async move {
            let async_result = run_async_operation(probe, commit, operation).await;
            let sessions_guard = sessions.lock().await;
            if let Some(session) = sessions_guard.get(&session_id) {
                let mut session = session.lock().await;
                if session.revision != operation_revision || session.outcome.is_some() {
                    return;
                }
                apply_async_outcome(&mut session, async_result);
                session.revision = bump_revision(session.revision);
            }
        });
        let session_guard = session.lock().await;
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
        let model =
            session
                .draft
                .models
                .first()
                .cloned()
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
            api_style: session.draft.api_style.clone(),
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
}

/// 后台执行探测；结果由调用方写回 session。
async fn run_probe(
    probe: std::sync::Arc<dyn ProviderProbePort>,
    request: ProviderProbeRequest,
) -> AsyncOutcome {
    match probe.probe(request).await {
        Ok(result) => AsyncOutcome::ProbeSuccess {
            latency_ms: result.latency.as_millis() as u64,
        },
        Err(error) => AsyncOutcome::ProbeFailed {
            kind: error.kind,
            message: error.message,
        },
    }
}

/// 后台执行提交；结果由调用方写回 session。
async fn run_commit(
    commit: Option<std::sync::Arc<dyn ConnectCommitPort>>,
    request: ConnectCommitRequest,
) -> AsyncOutcome {
    let Some(commit) = commit else {
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

/// 后台执行探测 / 提交；结果由调用方写回 session（revision 校验）。
async fn run_async_operation(
    probe: std::sync::Arc<dyn ProviderProbePort>,
    commit: Option<std::sync::Arc<dyn ConnectCommitPort>>,
    operation: AsyncOperation,
) -> AsyncOutcome {
    match operation {
        AsyncOperation::Probe(request) => run_probe(probe, request).await,
        AsyncOperation::Commit(request) => run_commit(commit, request).await,
    }
}

fn apply_async_outcome(session: &mut ConnectSession, outcome: AsyncOutcome) {
    match outcome {
        AsyncOutcome::ProbeSuccess { latency_ms } => {
            // 成功同样停在结果页等待用户回车确认（ContinueAfterProbe）
            // 才进入 Review，与失败路径一致。
            session.probe_status = Some(ProbeStatusView::Success { latency_ms });
            session.stage = ConnectStage::Probing;
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

impl ConnectAppService {
    /// 同步 handler。把 stage 修改 / draft 修改直接写入 session，
    /// 返回 (Option<error>, SyncOutcome)。
    fn handle_sync(
        &self,
        session: &mut ConnectSession,
        command: &crate::connect::ConnectCommand,
    ) -> (Option<ConnectError>, SyncOutcome) {
        use crate::connect::ConnectCommand as Cmd;
        match command {
            Cmd::Back => self.sync_back(session),
            Cmd::SelectProvider { source } => self.sync_select_provider(session, source.clone()),
            Cmd::BeginCustomProvider => {
                session.stage = ConnectStage::EditCustomProvider;
                (None, SyncOutcome::Proceed)
            }
            Cmd::SelectCustomProvider {
                name,
                driver,
                base_url,
            } => self.sync_select_custom_provider(session, name, driver, base_url),
            Cmd::ConfirmOverwrite => self.sync_confirm_overwrite(session),
            Cmd::RejectOverwrite => self.sync_reject_overwrite(session),
            Cmd::SetEndpoint {
                base_url,
                api_style,
            } => self.sync_set_endpoint(session, base_url, api_style.as_deref()),
            Cmd::SetCredential { api_key } => self.sync_set_credential(session, api_key),
            Cmd::SetProviderUserAgent { raw } => {
                self.sync_set_provider_user_agent(session, raw.as_deref())
            }
            Cmd::SetSelectedModels { models } => self.sync_set_selected_models(session, models),
            Cmd::EnterCustomModel { target_model } => {
                self.sync_enter_custom_model(session, target_model.clone())
            }
            Cmd::UpsertCustomModel {
                model,
                set_as_default,
            } => self.sync_upsert_custom_model(session, model, *set_as_default),
            Cmd::SkipProbe => self.sync_skip_probe(session),
            Cmd::BeginProbe => (None, SyncOutcome::Proceed), // handled in async
            Cmd::ContinueAfterProbe => self.sync_continue_after_probe(session),
            Cmd::EditAfterProbeFailure => self.sync_edit_after_probe_failure(session),
            Cmd::ConfirmSave => (None, SyncOutcome::Proceed), // handled in async
        }
    }

    fn catalog_entry(&self, source: &ProviderSource) -> Option<&'static ProviderCatalogEntry> {
        self.catalog.iter().find(|entry| &entry.source == source)
    }

    fn sync_back(&self, session: &mut ConnectSession) -> (Option<ConnectError>, SyncOutcome) {
        session.stage = match session.stage {
            ConnectStage::ConfirmOverwrite | ConnectStage::EditEndpoint => {
                ConnectStage::SelectProvider
            }
            ConnectStage::EditCredential => ConnectStage::EditEndpoint,
            ConnectStage::EditUserAgent => ConnectStage::EditCredential,
            ConnectStage::SelectModel => ConnectStage::EditUserAgent,
            ConnectStage::EditCustomModel => ConnectStage::SelectModel,
            ConnectStage::ChooseProbe => ConnectStage::SelectModel,
            // 探测中/失败后返回：回到测试选择页（可重新测试或跳过），
            // 而不是 InvalidTransition 导致表单整体退出。
            ConnectStage::Probing => ConnectStage::ChooseProbe,
            ConnectStage::Review => ConnectStage::ChooseProbe,
            actual => {
                return (
                    Some(ConnectError::InvalidTransition {
                        command: "Back",
                        actual,
                    }),
                    SyncOutcome::Proceed,
                );
            }
        };
        session.last_error = None;
        session.probe_status = None;
        (None, SyncOutcome::Proceed)
    }

    fn sync_select_custom_provider(
        &self,
        session: &mut ConnectSession,
        name: &str,
        driver: &str,
        base_url: &str,
    ) -> (Option<ConnectError>, SyncOutcome) {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return (
                Some(ConnectError::Validation {
                    field: "provider_name",
                    reason: "Provider 名称不能为空".to_string(),
                }),
                SyncOutcome::Proceed,
            );
        }
        let Some(entry) = crate::catalog::find_by_driver(driver.trim()) else {
            return (
                Some(ConnectError::Validation {
                    field: "driver",
                    reason: format!("未知 driver：{}", driver.trim()),
                }),
                SyncOutcome::Proceed,
            );
        };
        match ConnectDraft::normalize_base_url(base_url) {
            Ok(url) => {
                session.draft.source = Some(ProviderSource::new_owned(trimmed_name.to_string()));
                session.draft.driver = Some(entry.driver);
                session.draft.base_url = Some(url);
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

    fn sync_select_provider(
        &self,
        session: &mut ConnectSession,
        source: ProviderSource,
    ) -> (Option<ConnectError>, SyncOutcome) {
        // catalog 外的自定义已有 source（如用户此前完全自定义配置的
        // OmniRoute 等）同样允许：命中快照即走覆盖确认与已有值预填。
        let catalog_entry = self.catalog_entry(&source);
        if catalog_entry.is_none() && !session.existing_providers.contains_key(source.as_str()) {
            return (
                Some(ConnectError::CatalogUnavailable {
                    reason: format!("未知 source: {}", source.as_str()),
                }),
                SyncOutcome::Proceed,
            );
        }
        let source_key = source.as_str().to_string();
        if let Some(entry) = catalog_entry {
            session.draft.driver = Some(entry.driver);
            if session.draft.base_url.is_none() {
                if let Some(endpoint) = entry.default_endpoint {
                    session.draft.base_url = Some(endpoint.url.to_string());
                }
            }
        }
        session.draft.source = Some(source);
        // 已有 Provider 判断走 start 时加载的内存快照（路径无关）。
        if let Some(existing) = session.existing_providers.remove(&source_key) {
            session.existing_provider = Some(existing);
            session.stage = ConnectStage::ConfirmOverwrite;
            return (None, SyncOutcome::Proceed);
        }
        session.stage = ConnectStage::EditEndpoint;
        (None, SyncOutcome::Proceed)
    }

    fn sync_confirm_overwrite(
        &self,
        session: &mut ConnectSession,
    ) -> (Option<ConnectError>, SyncOutcome) {
        session.stage = ConnectStage::EditEndpoint;
        // ConfirmOverwrite 之后，draft 默认值来自全局配置的已有 Provider 配置
        //（endpoint / UA / 模型），credential 标为 PreservedFromExisting；
        // 若用户再调 SetCredential，状态会切到 UserSet/NotSet。
        if let Some(provider) = session.existing_provider.as_ref() {
            if !provider.base_url.trim().is_empty() {
                session.draft.base_url = Some(provider.base_url.clone());
            }
            // 自定义已有 source 无 catalog 条目；driver 从快照补齐，
            // 供 endpoint 页接口风格判定与 probe / commit 使用。
            if session.draft.driver.is_none() {
                if let Some(driver) = provider.driver.as_ref().and_then(|d| d.as_known()) {
                    session.draft.driver = Some(*driver);
                }
            }
            if provider.user_agent.is_some() {
                session.draft.provider_user_agent = provider.user_agent.clone();
            }
            session.draft.credential_mask = provider.credential_mask.clone();
            let models: Vec<ModelDraft> = provider
                .models
                .iter()
                .map(|model| ModelDraft {
                    model_id: model.model_id.clone(),
                    context_window: model.context_window,
                    max_tokens: model.max_tokens,
                    reasoning_effort: model.reasoning_effort.clone(),
                })
                .filter(|model| model.validate().is_ok())
                .collect();
            session.draft.models = models;
            if matches!(
                provider.api_key_status,
                super::states::ExistingCredentialStatus::Present
            ) {
                session.draft.preserve_existing_credential();
                session.draft.preserved_api_key = provider.api_key.clone();
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
        session.draft.credential_mask = None;
        session.draft.preserved_api_key = None;
        (None, SyncOutcome::Proceed)
    }

    fn sync_set_endpoint(
        &self,
        session: &mut ConnectSession,
        base_url: &str,
        api_style: Option<&str>,
    ) -> (Option<ConnectError>, SyncOutcome) {
        match ConnectDraft::normalize_base_url(base_url) {
            Ok(value) => {
                session.draft.base_url = Some(value);
                session.draft.api_style = api_style
                    .map(str::trim)
                    .filter(|style| !style.is_empty())
                    .map(str::to_string);
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
        // 空提交保持现状：已有保留 key（PreservedFromExisting）不因掩码预填下
        // 直接回车而丢失；显式清除凭证走配置删除而非向导。
        if !api_key.is_empty() {
            session.draft.set_user_credential(api_key.to_string());
        }
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

    fn sync_set_selected_models(
        &self,
        session: &mut ConnectSession,
        models: &[ModelDraft],
    ) -> (Option<ConnectError>, SyncOutcome) {
        if models.is_empty() {
            return (
                Some(ConnectError::Validation {
                    field: "model",
                    reason: "至少选择一个模型".to_string(),
                }),
                SyncOutcome::Proceed,
            );
        }
        for model in models {
            if let Err(err) = model.validate() {
                return (
                    Some(ConnectError::Validation {
                        field: "model",
                        reason: format!("模型 {}：{}", model.model_id, err.message()),
                    }),
                    SyncOutcome::Proceed,
                );
            }
        }
        session.draft.models = models.to_vec();
        session.stage = ConnectStage::ChooseProbe;
        (None, SyncOutcome::Proceed)
    }

    fn sync_enter_custom_model(
        &self,
        session: &mut ConnectSession,
        target_model: Option<String>,
    ) -> (Option<ConnectError>, SyncOutcome) {
        session.draft.editing_model_id = target_model;
        session.stage = ConnectStage::EditCustomModel;
        (None, SyncOutcome::Proceed)
    }

    fn sync_upsert_custom_model(
        &self,
        session: &mut ConnectSession,
        model: &ModelDraft,
        set_as_default: bool,
    ) -> (Option<ConnectError>, SyncOutcome) {
        if let Err(err) = model.validate() {
            return (
                Some(ConnectError::Validation {
                    field: "model",
                    reason: err.message().to_string(),
                }),
                SyncOutcome::Proceed,
            );
        }
        match session
            .draft
            .models
            .iter_mut()
            .find(|existing| existing.model_id == model.model_id)
        {
            Some(existing) => *existing = model.clone(),
            None => session.draft.models.push(model.clone()),
        }
        // 全局默认唯一：设为默认替换旧值；取消且目标即当前默认时清除。
        if set_as_default {
            session.draft.default_model_id = Some(model.model_id.clone());
        } else if session.draft.default_model_id.as_deref() == Some(model.model_id.as_str()) {
            session.draft.default_model_id = None;
        }
        // 返回模型页：允许继续添加 / 调整勾选后再提交。
        session.stage = ConnectStage::SelectModel;
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

    fn resolve_user_agent(&self, draft: &ConnectDraft) -> String {
        resolve_provider_user_agent_str(assemble_provider_user_agent_inputs(
            ProviderUserAgentRequest {
                provider_user_agent: draft.provider_user_agent.as_deref(),
                source_key: draft.source.as_ref().map(ProviderSource::as_str),
                driver: draft.driver.map(DriverId::as_str),
                global_user_agent: self.global_user_agent.as_deref(),
            },
            self.system.clone(),
            self.version,
        ))
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
            existing_providers: session
                .existing_providers
                .values()
                .map(Into::into)
                .collect(),
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
        source: draft.source.clone(),
        driver: draft.driver,
        base_url: draft.base_url.clone(),
        api_style: draft.api_style.clone(),
        has_api_key: draft.has_api_key(),
        provider_user_agent: draft.provider_user_agent.clone(),
        credential_mask: draft.credential_mask.clone(),
        editing_model_id: draft.editing_model_id.clone(),
        models: draft
            .models
            .iter()
            .map(|model| ModelDraftView {
                model_id: model.model_id.clone(),
                context_window: Some(model.context_window),
                max_tokens: Some(model.max_tokens),
                reasoning_effort: model.reasoning_effort.clone(),
            })
            .collect(),
        default_model_id: draft.default_model_id.clone(),
    }
}
