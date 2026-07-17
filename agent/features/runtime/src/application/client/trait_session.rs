//! session 相关方法实际逻辑。

use std::sync::Arc;

use sdk::{SdkError, SessionSummary};

use super::accessors::{AgentClientImpl, RuntimeHandle};
use super::mapping;

/// Temporary pre-#890 persistence facade: project the authoritative Task ACL
/// read model into the legacy Session DTO without exposing TaskStore snapshots.
/// This disappears when TaskPersist owns the Session integration.
fn legacy_task_snapshot_from_access(
    access: &dyn task::TaskAccess,
) -> Option<share::task::TaskSnapshot> {
    // TaskAccess intentionally exposes owned read models rather than a capture
    // API. Revision bracketing gives this short-lived facade a coherent view
    // without publishing TaskStore capture/install ahead of #890.
    loop {
        let before = access.revision();
        let tasks = access.list();
        let batches = access.list_batches();
        let current_batch = access.current_batch();
        if access.revision() == before {
            return legacy_task_snapshot(tasks, batches, current_batch);
        }
    }
}

fn legacy_task_snapshot(
    tasks: Vec<task::Task>,
    batches: Vec<task::Batch>,
    current_batch: Option<task::BatchId>,
) -> Option<share::task::TaskSnapshot> {
    if tasks.is_empty() && batches.is_empty() {
        return None;
    }

    let next_id = tasks
        .iter()
        .map(|task| task.id().get())
        .max()
        .map_or(1, |id| {
            id.checked_add(1)
                .expect("Task aggregate guarantees an allocatable ID after every live Task")
        });
    Some(share::task::TaskSnapshot {
        tasks: tasks
            .into_iter()
            .map(|task| share::task::Task {
                id: task.id().get().to_string(),
                subject: task.subject().to_owned(),
                description: task.description().to_owned(),
                status: match task.status() {
                    task::TaskStatus::Pending => share::task::TaskStatus::Pending,
                    task::TaskStatus::InProgress => share::task::TaskStatus::InProgress,
                    task::TaskStatus::Completed => share::task::TaskStatus::Completed,
                    task::TaskStatus::Deleted => share::task::TaskStatus::Deleted,
                },
                owner: None,
                blocked_by: task
                    .blocked_by()
                    .iter()
                    .map(|id| id.get().to_string())
                    .collect(),
                priority: match task.priority() {
                    task::TaskPriority::Low => share::task::TaskPriority::Low,
                    task::TaskPriority::Normal => share::task::TaskPriority::Normal,
                    task::TaskPriority::High => share::task::TaskPriority::High,
                    task::TaskPriority::Urgent => share::task::TaskPriority::Urgent,
                },
                created_at: task.created_at(),
                updated_at: task.updated_at(),
                session_id: task.session_id().map(str::to_owned),
                batch: task.batch().get(),
            })
            .collect(),
        next_id,
        current_batch: current_batch.map_or(0, task::BatchId::get),
        batches: batches
            .into_iter()
            .map(|batch| share::task::Batch {
                id: batch.id().get(),
                summary: batch.summary().map(str::to_owned),
                status: match batch.status() {
                    task::BatchStatus::Active => share::task::BatchStatus::Active,
                    task::BatchStatus::Paused => share::task::BatchStatus::Paused,
                    task::BatchStatus::Archived => share::task::BatchStatus::Archived,
                },
                created_at: batch.created_at(),
                last_active_turn: batch.last_active_turn(),
                silence_turns: batch.silence_turns(),
            })
            .collect(),
    })
}

/// 从 chain + handle 数据保存 session。
/// loop 内部直接传入 chain（无需经过 inner.current_chain）。
pub(super) async fn save_chain_to_handle(
    chain: &context::session::ChatChain,
    inner: &Arc<RuntimeHandle>,
) -> Result<(), SdkError> {
    // 从 chain 取活跃段（真实 segment 边界）
    let active_segments: Vec<context::session::ChatSegment> = chain.active_segments().to_vec();
    let task_snapshot =
        legacy_task_snapshot_from_access(inner.context.resources.task_access.as_ref());
    let workspace = Some(inner.workspace.persist().snapshot());
    let frozen_chats = inner
        .frozen_chats
        .lock()
        .map_err(|_| SdkError::Internal("frozen_chats 锁已损坏".to_string()))?
        .clone();

    // 构造 chats = 旧链（冻结）+ 活跃段
    let mut chats = frozen_chats;
    chats.extend(active_segments);

    let mut session = context::session::Session::new(
        inner.session_id.clone(),
        inner.cwd.to_string_lossy().to_string(),
    );
    session.chats = chats;
    // 旧 messages 字段置空（已迁移到 chats）
    session.messages = Vec::new();
    session.updated_at = context::session::now_iso();
    session.metadata.model = Some(mapping::model_display(
        &inner.resolved_model.source_key,
        &inner.resolved_model.model.name,
        &inner.resolved_model.model.id,
    ));
    session.tasks = task_snapshot;
    session.workspace = workspace;
    context::session::save_session(&session)
        .await
        .map_err(SdkError::Session)
}

/// 兼容：从 inner.current_chain 读 chain 后调 save_chain_to_handle。
/// 仅供 loop 外部路径使用。
pub(super) async fn save_session_from_handle(inner: &Arc<RuntimeHandle>) -> Result<(), SdkError> {
    let chain = inner
        .current_chain
        .lock()
        .map_err(|_| SdkError::Internal("当前 session chain 锁已损坏".to_string()))?
        .clone();
    save_chain_to_handle(&chain, inner).await
}

pub(super) async fn list_sessions_impl(
    _me: &AgentClientImpl,
) -> Result<Vec<SessionSummary>, SdkError> {
    Ok(context::session::list_sessions()
        .await
        .into_iter()
        .map(mapping::session_summary_from_runtime)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use task::TaskAccess;
    #[test]
    fn legacy_snapshot_projects_authoritative_tool_created_task() {
        let access = task::TaskStore::new();
        let batch = access
            .create_batch(
                task::BatchCreateSpec::try_new("request".into()).unwrap(),
                10,
            )
            .unwrap();
        let created = access
            .create_task(
                task::TaskCreateSpec::try_new(
                    "created by tool".into(),
                    "details".into(),
                    None,
                    task::TaskPriority::High,
                )
                .unwrap(),
                11,
            )
            .unwrap();

        let snapshot = legacy_task_snapshot_from_access(&access).expect("non-empty snapshot");
        assert_eq!(snapshot.tasks.len(), 1);
        assert_eq!(snapshot.tasks[0].id, created.value.id().get().to_string());
        assert_eq!(snapshot.tasks[0].owner, None);
        assert_eq!(snapshot.next_id, created.value.id().get() + 1);
        assert_eq!(snapshot.current_batch, batch.value.id().get());
        assert_eq!(snapshot.batches.len(), 1);
    }
}
