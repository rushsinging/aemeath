//! Main Session 切换协调模块（#2025 迁移自 context）。
//!
//! resume 编排归 runtime：发起、时序、回执本就在 runtime；协调者持有
//! 切换协议的全部编排参与方（Project/Config/Memory/TaskData/Context），
//! 只开口协议方法，NEVER 暴露依赖转发访问器。

mod bound;
mod coordinator;
mod error;
mod gate;
mod resume;

pub use bound::BoundMainRun;
pub use coordinator::{
    SessionSwitchCoordinator, SessionSwitchCoordinatorBuilder, SessionSwitchDependencies,
};
pub use error::MainSessionError;

use std::sync::Arc;

use config::{PreparedProjectConfigData, ProjectConfigLocationData};
use context::session::{CanonicalSession, SnapshotState};
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
    let now = context::session::now_iso();
    let initial_session = CanonicalSession {
        id: context::session::new_session_id(),
        chats: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Captured(workspace_snapshot),
        revision: 0,
        compact: None,
        cleared_after: None,
        run_slices: Default::default(),
        committed_steps: Default::default(),
        skill_load_records: Vec::new(),
    };

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
            task_persist: deps.task_persist,
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
