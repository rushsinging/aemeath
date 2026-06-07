use std::sync::atomic::Ordering;

use sdk::{ChangeSet, CostInfo, ProjectContext, SdkError, SessionSnapshot, TaskStatusView};
use tokio::sync::watch;

use super::accessors::AgentClientImpl;
use storage::api::TaskStatus;

type Result<T> = std::result::Result<T, SdkError>;

pub(super) fn session_snapshot_impl(me: &AgentClientImpl) -> SessionSnapshot {
    SessionSnapshot {
        id: me.inner.session_id.clone(),
        message_count: 0, // TODO: 从实际 session 获取
        total_tokens: 0,
        messages: vec![],
        created_at: None,
        trimmed: 0,
        repaired: 0,
        workspace: None,
        tasks: None,
    }
}

pub(super) fn cost_impl(_me: &AgentClientImpl) -> CostInfo {
    // TODO: 从 cost_tracker 获取
    CostInfo::default()
}

pub(super) fn task_list_impl(_me: &AgentClientImpl) -> Vec<sdk::TaskSummary> {
    Vec::new()
}

pub(super) async fn task_status_impl(me: &AgentClientImpl) -> Result<TaskStatusView> {
    let tasks = me.inner.context.task_store.list_current_batch().await;
    let active: Vec<_> = tasks
        .iter()
        .filter(|t| t.status != TaskStatus::Deleted)
        .cloned()
        .collect();
    if active.is_empty() {
        return Ok(TaskStatusView::default());
    }

    let display_map = me.inner.context.task_store.get_batch_display_map().await;
    let max_lines = share::config::TaskListConfig::default().max_lines;
    let lines = super::mapping::task_status_lines(&active, &display_map, max_lines);
    Ok(TaskStatusView { lines })
}

pub(super) fn project_impl(me: &AgentClientImpl) -> ProjectContext {
    let workspace = project::api::WorkspacePersist::snapshot(me.inner.workspace.as_ref());
    let cwd = workspace.path_base.clone();
    let path_base = workspace.path_base.clone();
    let working_root = workspace.working_root.clone();
    let git_branch = project::api::GitWorktreeOps::current_branch(
        &project::api::GitCli,
        std::path::Path::new(&path_base),
    )
    .ok()
    .flatten();

    ProjectContext {
        cwd,
        path_base,
        working_root,
        git_branch,
    }
}

pub(super) fn changes_impl(me: &AgentClientImpl) -> watch::Receiver<ChangeSet> {
    me.inner.change_rx.clone()
}

pub(super) fn cancel_impl(me: &AgentClientImpl) {
    me.inner.cancel_token.store(true, Ordering::Release);
    if let Ok(guard) = me.inner.current_cancel.lock() {
        if let Some(token) = guard.as_ref() {
            token.cancel();
        }
    }
}

pub(super) fn set_current_turn_impl(_me: &AgentClientImpl, turn: usize) {
    crate::utils::bootstrap::set_current_turn(turn);
}

pub(super) async fn restore_tasks_impl(
    me: &AgentClientImpl,
    snapshot: serde_json::Value,
) -> Result<()> {
    if let Some(ref store) = me.inner.task_store {
        if let Ok(task_snapshot) = serde_json::from_value(snapshot) {
            store.restore(task_snapshot).await;
            me.notify_change(ChangeSet::TASKS);
        }
    }
    Ok(())
}

pub(super) async fn clear_tasks_impl(me: &AgentClientImpl) -> Result<()> {
    if let Some(ref store) = me.inner.task_store {
        store.clear().await;
        me.notify_change(ChangeSet::TASKS);
    }
    Ok(())
}
