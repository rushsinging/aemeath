//! Main Session 切换协调者：持有切换协议的全部编排参与方，
//! 只开口协议方法、NEVER 暴露依赖转发访问器（#2025）。
//!
//! 平移自 context `application/main_session.rs` 的协议主体：行为语义
//! 等价（bind = shared permit，resume = exclusive permit，NEVER
//! shared→exclusive 升级），宿主归 runtime——resume 的发起、时序与回执
//! 本就在 runtime，编排参与方随协议携带。

use std::sync::{Arc, RwLock as StdRwLock};

use config::{ConfigReader, ProjectConfigParticipant};
use context::session::CanonicalSession;
use context::MainContextFactory;
use context::SessionManagementError;
use context::SessionManagementPort;
use memory::api::{MemoryOpener, MemoryPort};
use project::{WorkspaceReader, WorkspaceWriter};
use share::config::domain::snapshot::ConfigSnapshot;
use share::session_types::ProjectIdentityData;
use task::TaskRestoreAccess;

use super::bound::BoundMainRun;
use super::gate::{SessionSwitchClosed, SessionSwitchGate};

/// 装配 [`SessionSwitchCoordinator`] 所需的全部跨 BC 参与方。
///
/// composition 注入各 BC port；协调者持有它们仅供切换协议内部使用。
pub struct SessionSwitchDependencies {
    pub workspace: project::Workspace,
    pub task_restore: Arc<dyn TaskRestoreAccess>,
    pub config_reader: Arc<dyn ConfigReader>,
    pub config_participant: Arc<dyn ProjectConfigParticipant>,
    pub memory_opener: Box<dyn MemoryOpener>,
    /// composition 注入的唯一 Session 管理 Port。协议只在 resume / metadata
    /// 更新时借用；Runtime 侧 Session 命令持同一 Arc。
    pub session_management: Arc<dyn SessionManagementPort>,
    pub context_factory: Arc<MainContextFactory>,
}

/// 带初始 committed 状态的完整装配入参（测试与 bootstrap 共用）。
pub struct SessionSwitchCoordinatorBuilder {
    pub workspace_read: Arc<dyn WorkspaceReader>,
    pub workspace_persist: Arc<dyn WorkspaceWriter>,
    pub task_restore: Arc<dyn TaskRestoreAccess>,
    pub config_reader: Arc<dyn ConfigReader>,
    pub config_participant: Arc<dyn ProjectConfigParticipant>,
    pub memory_opener: Box<dyn MemoryOpener>,
    pub session_management: Arc<dyn SessionManagementPort>,
    pub initial_session: CanonicalSession,
    pub initial_memory: Arc<dyn MemoryPort>,
    pub context_factory: Arc<MainContextFactory>,
}

/// Main Session 切换协调者。
///
/// 职责边界：`gate` + committed 四元组（session / memory / context /
/// config snapshot）的原子切换协议。config / workspace / task port 仅在
/// 协议内部使用（prepare / commit），NEVER 向外转发。
pub struct SessionSwitchCoordinator {
    pub(super) gate: SessionSwitchGate,
    pub(super) workspace_read: Arc<dyn WorkspaceReader>,
    pub(super) workspace_persist: Arc<dyn WorkspaceWriter>,
    pub(super) task_restore: Arc<dyn TaskRestoreAccess>,
    pub(super) config_reader: Arc<dyn ConfigReader>,
    pub(super) config_participant: Arc<dyn ProjectConfigParticipant>,
    pub(super) memory_opener: Box<dyn MemoryOpener>,
    pub(super) session_management: Arc<dyn SessionManagementPort>,
    pub(super) committed_session: Arc<StdRwLock<Arc<CanonicalSession>>>,
    pub(super) committed_memory: Arc<StdRwLock<Arc<dyn MemoryPort>>>,
    pub(super) pending_session_restart_revision:
        Arc<StdRwLock<Option<share::config::domain::snapshot::ConfigRevision>>>,
    pub(super) context: context::ContextPorts,
    pub(super) mutation_gate: Arc<tokio::sync::Mutex<()>>,
}

impl SessionSwitchCoordinator {
    /// 从跨 BC 依赖与初始 committed 状态组装协调者。
    pub fn assemble(builder: SessionSwitchCoordinatorBuilder) -> Self {
        let committed_session = Arc::new(StdRwLock::new(Arc::new(builder.initial_session)));
        let committed_memory = Arc::new(StdRwLock::new(builder.initial_memory));
        let mutation_gate = Arc::new(tokio::sync::Mutex::new(()));
        let context = builder.context_factory.build(
            Arc::clone(&committed_session),
            Arc::clone(&committed_memory),
            Arc::clone(&mutation_gate),
        );
        let pending_session_restart_revision = Arc::new(StdRwLock::new(None));
        Self {
            gate: SessionSwitchGate::new(),
            workspace_read: builder.workspace_read,
            workspace_persist: builder.workspace_persist,
            task_restore: builder.task_restore,
            config_reader: builder.config_reader,
            config_participant: builder.config_participant,
            memory_opener: builder.memory_opener,
            session_management: builder.session_management,
            committed_session,
            committed_memory,
            pending_session_restart_revision,
            context,
            mutation_gate,
        }
    }

    /// 测试钩子：以注入的 ContextPorts（如记录型装饰器）替代 factory 产物组装。
    #[cfg(any(test, feature = "dev"))]
    pub fn assemble_with_ports(
        builder: SessionSwitchCoordinatorBuilder,
        context: context::ContextPorts,
    ) -> Self {
        let committed_session = Arc::new(StdRwLock::new(Arc::new(builder.initial_session)));
        let committed_memory = Arc::new(StdRwLock::new(builder.initial_memory));
        let mutation_gate = Arc::new(tokio::sync::Mutex::new(()));
        let pending_session_restart_revision = Arc::new(StdRwLock::new(None));
        Self {
            gate: SessionSwitchGate::new(),
            workspace_read: builder.workspace_read,
            workspace_persist: builder.workspace_persist,
            task_restore: builder.task_restore,
            config_reader: builder.config_reader,
            config_participant: builder.config_participant,
            memory_opener: builder.memory_opener,
            session_management: builder.session_management,
            committed_session,
            committed_memory,
            pending_session_restart_revision,
            context,
            mutation_gate,
        }
    }

    // ── 协议最小读取 ──

    /// 当前 committed canonical session（cheap Arc clone）。
    pub fn committed_session(&self) -> Arc<CanonicalSession> {
        Arc::clone(&self.committed_session.read().unwrap())
    }

    /// 当前 committed config snapshot（四元组读取，语义同 context 侧直读）。
    pub fn committed_config_snapshot(&self) -> ConfigSnapshot {
        self.config_reader.committed_snapshot()
    }

    /// 当前 committed Context port。
    pub fn committed_context(&self) -> &context::ContextPorts {
        &self.context
    }

    /// 当前 committed memory port。
    pub fn committed_memory(&self) -> Arc<dyn MemoryPort> {
        Arc::clone(&self.committed_memory.read().unwrap())
    }

    /// 协议内使用的项目身份（resume 按 project 加载 / 校验）。
    pub(crate) fn project_identity(&self) -> ProjectIdentityData {
        self.workspace_read.project_identity()
    }

    /// 标记需要重启会话以应用新 config revision。
    pub fn mark_session_restart_required(
        &self,
        revision: share::config::domain::snapshot::ConfigRevision,
    ) {
        *self.pending_session_restart_revision.write().unwrap() = Some(revision);
    }

    /// 读取待应用的重启 revision（无则 `None`）。
    pub fn pending_session_restart_revision(
        &self,
    ) -> Option<share::config::domain::snapshot::ConfigRevision> {
        *self.pending_session_restart_revision.read().unwrap()
    }

    // ── shared-permit 协议 ──

    /// 非阻塞取 shared permit：Run 创建（含 sub-run）准入尝试，
    /// 切换进行中返回 `Err`（对齐 context 侧 gate 语义）。
    pub fn try_acquire_shared(
        &self,
    ) -> Result<super::gate::OwnedSessionSharedPermit, super::gate::SessionSwitchInProgress> {
        self.gate.try_acquire_shared()
    }

    /// shared permit 内执行读取：idle 期磁盘 session 操作（如 `/session export`）
    /// 的最小 gate-aware 读路径，防止与 resume 竞态。
    pub async fn with_shared<F, R>(&self, f: F) -> Result<R, SessionSwitchClosed>
    where
        F: std::future::Future<Output = R>,
    {
        let _permit = self.gate.acquire_shared().await?;
        Ok(f.await)
    }

    /// 捕获 committed 四元组（session / memory / context / config snapshot）。
    ///
    /// 返回的 [`BoundMainRun`] 生命周期内持有 shared permit，阻塞
    /// resume（exclusive）直到所有绑定 drop；四个字段同一瞬间读取，
    /// 相互一致。
    pub async fn bind_run(&self) -> Result<BoundMainRun, SessionSwitchClosed> {
        let permit = self.gate.acquire_shared().await?;
        let session = self.committed_session();
        let memory = self.committed_memory();
        let config = self.config_reader.committed_snapshot();
        Ok(BoundMainRun::new(
            permit,
            session,
            self.context.clone(),
            memory,
            config,
        ))
    }

    // ── metadata 协议（mutation_gate 串行化）──

    /// 原子更新活跃会话 metadata（覆盖式：调用方读现值-改字段-回传整份），
    /// 先落盘再回读合并 revision/updated_at，保留内存里其它 committed 内容。
    pub async fn update_active_session_metadata(
        &self,
        update: context::session::SessionMetadata,
    ) -> Result<(), SessionManagementError> {
        let _mutation = self.mutation_gate.lock().await;
        let before = self.committed_session();
        let session_id = before.id.clone();
        let project = self.project_identity();

        if update == before.metadata {
            return Ok(());
        }

        self.session_management
            .update_metadata_for_project(&session_id, &project, update)
            .await?;

        let disk = self
            .session_management
            .load_for_project(&session_id, &project)
            .await?;
        if disk.id != before.id {
            return Err(SessionManagementError::Storage(format!(
                "active session id 在 metadata 更新后不一致: before={}, disk={}",
                before.id, disk.id
            )));
        }

        let mut merged = (*before).clone();
        merged.revision = disk.revision;
        merged.updated_at = disk.updated_at;
        merged.metadata = disk.metadata;
        *self
            .committed_session
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(merged);
        Ok(())
    }
}
