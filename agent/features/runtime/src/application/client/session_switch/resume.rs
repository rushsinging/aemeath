//! Main Session 切换协议实现：加载 → 4-BC prepare → 同步 commit → CAS。
//!
//! 平移自 context `application/main_session.rs::resume_prepared` 与
//! `adapters/session_resume.rs::resume_session`（#2025）：prepare 全部成功
//! 才 commit，任一失败 committed 四元组不动。

use std::sync::Arc;
use std::time::Instant;

use config::{PreparedProjectConfigData, ProjectConfigLocationData};
use context::session::{same_project_identity, CanonicalSession, SessionRestore, SnapshotState};
use context::{SessionManagementError, SessionResumeViewData};
use memory::api::ProjectMemoryKey;
use project::WorkspaceRestoreData;

use super::coordinator::SessionSwitchCoordinator;
use super::error::MainSessionError;
use crate::LOG_TARGET;

impl SessionSwitchCoordinator {
    /// resume 入口：加载 canonical session → 独占切换协议 → 组装 resume 视图。
    pub async fn resume_session(
        &self,
        session_id: &str,
    ) -> Result<SessionResumeViewData, SessionManagementError> {
        let started = Instant::now();
        let project = self.project_identity();
        log::debug!(
            target: LOG_TARGET,
            "resume_lifecycle boundary=runtime_session_switch stage=session_load_started session_id={}",
            session_id
        );
        let loaded = self
            .session_management
            .load_for_resume(session_id, &project)
            .await?;
        let display_history = loaded.display_history;
        let session = loaded.active_session;
        log::debug!(
            target: LOG_TARGET,
            "resume_lifecycle boundary=runtime_session_switch stage=session_load_completed session_id={} committed_steps={} elapsed_ms={}",
            session_id,
            session.committed_steps.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        self.resume_prepared(session)
            .await
            .map_err(|error| SessionManagementError::Resume(error.to_string()))?;
        let restore_started = Instant::now();
        let committed = self.committed_session();
        let restore = if display_history.is_some() {
            SessionRestore::active_from_canonical(&committed)
        } else {
            SessionRestore::from_canonical(&committed)
        };
        log::debug!(
            target: LOG_TARGET,
            "resume_lifecycle boundary=runtime_session_switch stage=restore_completed session_id={} display_index_steps={} legacy_steps={} active_messages={} elapsed_ms={} restore_ms={}",
            committed.id,
            display_history.as_ref().map_or(0, |index| index.steps().len()),
            restore.display_steps.len(),
            restore.active_messages.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            restore_started.elapsed().as_secs_f64() * 1000.0
        );
        Ok(SessionResumeViewData {
            session_id: committed.id.clone(),
            active_messages: restore.active_messages,
            display_steps: restore.display_steps,
            display_history,
            compacted: restore.compacted,
            created_at: restore.created_at,
            trimmed: restore.trimmed,
            repaired: restore.repaired,
        })
    }

    /// 独占区切换协议：先 prepare 全部参与方（Project → Config → Memory →
    /// TaskData），全部成功后同步 commit；任一 prepare 失败保持旧状态。
    ///
    /// **禁止跨项目 resume**：持久化的 workspace 身份必须先匹配当前活跃
    /// 工作区身份。Git worktree 按 common-dir 匹配，非 git 项目按 canonical
    /// root 匹配。
    pub(super) async fn resume_prepared(
        &self,
        mut session: CanonicalSession,
    ) -> Result<(), MainSessionError> {
        let _exclusive = self
            .gate
            .acquire_owned_exclusive()
            .await
            .map_err(|_| MainSessionError::GateClosed)?;
        let _mutation = self.mutation_gate.lock().await;

        // ── 1. Project prepare ──
        // workspace 槽位必须携带类型化上下文；Missing/CapturedEmpty 无安全
        // 默认值，直接拒绝。
        let prepared_workspace: WorkspaceRestoreData = match &session.workspace {
            SnapshotState::Captured(dto) => match self.workspace_persist.prepare_restore(dto) {
                Ok(prepared) => prepared,
                Err(error) if error.message().contains("路径不存在") => {
                    let live_identity = self.workspace_read.project_identity();
                    if !same_project_identity(&live_identity, &dto.project_identity) {
                        return Err(MainSessionError::ProjectMismatch);
                    }
                    let live_workspace = self.workspace_persist.snapshot();
                    log::warn!(
                        target: LOG_TARGET,
                        "会话工作区已不存在，继续使用当前工作区"
                    );
                    session.workspace = SnapshotState::Captured(live_workspace.clone());
                    self.workspace_persist.prepare_restore(&live_workspace)?
                }
                Err(error) => return Err(error.into()),
            },
            SnapshotState::Missing | SnapshotState::CapturedEmpty => {
                return Err(MainSessionError::WorkspaceMissing);
            }
        };

        let prepared_identity = prepared_workspace.project_identity().clone();
        let live_identity = self.workspace_read.project_identity();
        if !same_project_identity(&live_identity, &prepared_identity) {
            return Err(MainSessionError::ProjectMismatch);
        }

        // ── 2. Config prepare ──
        let config_location = self.build_config_location(&prepared_identity)?;
        let prepared_config: PreparedProjectConfigData = self
            .config_participant
            .prepare_for_project(&config_location)
            .await
            .map_err(MainSessionError::ConfigPrepare)?;

        // ── 3. Memory eager open ──
        let memory_key = ProjectMemoryKey::derive(
            &prepared_identity.initial_cwd,
            prepared_identity.git_common_dir.as_deref(),
        )?;
        let new_memory: Arc<dyn memory::api::MemoryPort> = self
            .memory_opener
            .open_memory(&memory_key, prepared_config.memory_config())
            .await?;

        // ── 4. TaskData prepare ──
        // Missing/CapturedEmpty 映射到清空语义，commit 时清掉陈旧任务。
        // 经 TaskRestoreAccess 窄 port（runtime 不直接持 Task 持久化类型）。
        let prepared_task = match &session.tasks {
            SnapshotState::Captured(snapshot) => self
                .task_restore
                .prepare_session_restore(Some(snapshot))
                .map_err(MainSessionError::TaskRestore)?,
            SnapshotState::Missing | SnapshotState::CapturedEmpty => self
                .task_restore
                .prepare_session_restore(None)
                .map_err(MainSessionError::TaskRestore)?,
        };

        // ════════ 全部 prepare 成功 — commit 阶段（同步、无 await）════════

        self.workspace_persist.commit_restore(prepared_workspace);
        self.task_restore.commit_session_restore(prepared_task);

        // 发布新的 committed Session + Memory。其它 shared permit 持有者被
        // exclusive permit 阻塞，此写无竞争。
        let session_arc = Arc::new(session);
        *self.committed_session.write().unwrap() = Arc::clone(&session_arc);
        *self.committed_memory.write().unwrap() = Arc::clone(&new_memory);

        // config commit 最后：commit_project 更新 ConfigReader 内部 watch，
        // 后续 bind_run 看到新 config。
        self.config_participant
            .commit_project(prepared_config)
            .await;
        *self.pending_session_restart_revision.write().unwrap() = None;

        Ok(())
    }

    /// 从 prepared identity 推导 canonical project-config 位置。
    fn build_config_location(
        &self,
        identity: &share::session_types::ProjectIdentityData,
    ) -> Result<ProjectConfigLocationData, MainSessionError> {
        derive_config_location(identity)
    }
}

/// 从 [`share::session_types::ProjectIdentityData`] 推导 canonical
/// project-config 位置：search_root 取 identity 的 `initial_cwd`，
/// stable_identity 取 git 项目的 `git_common_dir`（非 git 取 `initial_cwd`），
/// 与 [`ProjectMemoryKey`] 的推导口径一致。
pub(super) fn derive_config_location(
    identity: &share::session_types::ProjectIdentityData,
) -> Result<ProjectConfigLocationData, MainSessionError> {
    let search_root = std::path::PathBuf::from(&identity.initial_cwd);
    let stable_identity: &[u8] = match identity.git_common_dir.as_deref() {
        Some(common) if !common.is_empty() => common.as_bytes(),
        _ => identity.initial_cwd.as_bytes(),
    };
    ProjectConfigLocationData::try_from_project_identity(search_root, stable_identity)
        .map_err(MainSessionError::ConfigLocation)
}
