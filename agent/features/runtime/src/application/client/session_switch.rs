//! Main Session 切换协调模块（#2025 迁移自 context）。
//!
//! resume 编排归 runtime：发起、时序、回执本就在 runtime；协调者持有
//! 切换协议的全部编排参与方（Project/Config/Memory/TaskData/Context），
//! 只开口协议方法，NEVER 暴露依赖转发访问器。

mod bound;
mod config_gate;
mod coordinator;
mod error;
mod gate;
mod resume;

pub use bound::BoundMainRun;
pub use coordinator::{
    SessionSwitchCoordinator, SessionSwitchCoordinatorBuilder, SessionSwitchDependencies,
};
pub use error::MainSessionError;
pub use gate::OwnedSessionSharedPermit;

use std::sync::Arc;

use config::{PreparedProjectConfigData, ProjectConfigLocationData};
use context::session::CanonicalSession;

use memory::api::ProjectMemoryKey;
use share::session_types::ProjectIdentityData;

/// Bootstrap 装配：从 live workspace 构造初始 canonical session，
/// prepare/commit 初始 config 位置，eager-open 初始 memory，再组装协调者。
///
/// workspace 身份是 config 位置的单一真相源，与 resume 协议同一推导口径。
pub async fn wire_session_switch_coordinator(
    deps: SessionSwitchDependencies,
) -> Result<Arc<SessionSwitchCoordinator>, MainSessionError> {
    let workspace_read = deps.workspace.read();
    let workspace_persist = deps.workspace.persist();
    let workspace_snapshot = workspace_persist.snapshot();
    // 快照经 empty + capture 两步组装（槽位字段 crate 私有）。
    let mut initial_session = CanonicalSession::empty(context::session::new_session_id());
    initial_session.capture_workspace(workspace_snapshot);

    // 从已验证的 workspace 身份 bootstrap 初始 config 位置：执行
    // prepare_for_project + commit_project，让活跃 project 位置从启动即就绪
    // ——后续 ConfigWriter 更新不依赖先发生一次 session resume。
    let identity: ProjectIdentityData = workspace_read.project_identity();
    let config_location: ProjectConfigLocationData = resume::derive_config_location(&identity)?;
    let prepared_config: PreparedProjectConfigData = deps
        .config_participant
        .prepare_for_project(&config_location)
        .await
        .map_err(MainSessionError::ConfigPrepare)?;
    let memory_config = prepared_config.memory_config().clone();
    deps.config_participant
        .commit_project(prepared_config)
        .await;

    // eager-open 初始 memory：workspace 身份 + project-scoped config 的
    // MemoryConfig（含持久化 override）。
    let memory_key =
        ProjectMemoryKey::derive(&identity.initial_cwd, identity.git_common_dir.as_deref())?;
    let initial_memory = deps
        .memory_opener
        .open_memory(&memory_key, &memory_config)
        .await?;

    Ok(Arc::new(SessionSwitchCoordinator::assemble(
        SessionSwitchCoordinatorBuilder {
            workspace_read,
            workspace_persist,
            task_restore: deps.task_restore,
            config_reader: deps.config_reader,
            config_participant: deps.config_participant,
            memory_opener: deps.memory_opener,
            session_management: deps.session_management,
            initial_session,
            initial_memory,
            context_factory: deps.context_factory,
        },
    )))
}

/// 测试装配：内存 opener + 调用方注入的 port 集（对齐 context
/// `test_support::wire_in_memory` 的形态，#2025 C2 切换配套）。
///
/// 生产 MUST NOT 使用；组装逻辑与 `wire_session_switch_coordinator`
/// 的 bootstrap 协议一致（initial canonical + config prepare/commit +
/// memory eager open）。
#[cfg(any(test, feature = "dev"))]
use config::{ConfigReader, ProjectConfigParticipant};
#[cfg(any(test, feature = "dev"))]
use context::{MainContextFactory, SessionManagementPort};

#[cfg(any(test, feature = "dev"))]
pub async fn wire_test_session_switch(
    workspace: &project::Workspace,
    task_restore: std::sync::Arc<task::TaskRestoreAccessAdapter>,
    config_reader: std::sync::Arc<dyn ConfigReader>,
    config_participant: std::sync::Arc<dyn ProjectConfigParticipant>,
    session_management: std::sync::Arc<dyn SessionManagementPort>,
    context_factory: std::sync::Arc<dyn MainContextFactory>,
) -> std::sync::Arc<SessionSwitchCoordinator> {
    wire_session_switch_coordinator(SessionSwitchDependencies {
        workspace: workspace.clone(),
        task_restore,
        config_reader,
        config_participant,
        memory_opener: Box::new(context::test_support::InMemoryTestOpener),
        session_management,
        context_factory,
    })
    .await
    .expect("test session switch wiring is infallible with in-memory opener")
}
