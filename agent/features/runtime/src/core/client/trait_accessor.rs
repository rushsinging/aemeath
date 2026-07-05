use sdk::{ChangeSet, CostInfo, ProjectContext, SdkError, SessionSnapshot};
use tokio::sync::watch;

use super::accessors::AgentClientImpl;

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

pub(super) fn project_impl(me: &AgentClientImpl) -> ProjectContext {
    let workspace = project::api::WorkspacePersist::snapshot(me.inner.workspace.as_ref());
    let cwd = workspace.path_base.clone();
    let path_base = workspace.path_base.clone();
    let workspace_root = workspace.workspace_root.clone();
    let git_branch = project::api::GitWorktreeOps::current_branch(
        &project::api::GitCli,
        std::path::Path::new(&path_base),
    )
    .ok()
    .flatten();

    ProjectContext {
        cwd,
        path_base,
        workspace_root,
        git_branch,
    }
}

pub(super) fn changes_impl(me: &AgentClientImpl) -> watch::Receiver<ChangeSet> {
    me.inner.change_rx.clone()
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
