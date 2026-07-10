//! session 相关方法实际逻辑。

use std::sync::Arc;

use sdk::{SdkError, SessionSummary};

use super::accessors::{AgentClientImpl, RuntimeHandle};
use super::mapping;

/// 从 RuntimeHandle 级别执行 save（不依赖 AgentClientImpl）。
/// 供 chat_impl spawn task 中 loop 退出后 auto-save 使用。
pub(super) async fn save_session_from_handle(inner: &Arc<RuntimeHandle>) -> Result<(), SdkError> {
    let messages = inner
        .current_messages
        .lock()
        .map_err(|_| SdkError::Internal("当前 session 消息锁已损坏".to_string()))?
        .clone();
    let task_snapshot = {
        let snap = inner.context.resources.task_store.snapshot().await;
        if snap.tasks.is_empty() {
            None
        } else {
            Some(snap)
        }
    };
    let workspace = Some(project::api::WorkspacePersist::snapshot(
        inner.workspace.as_ref(),
    ));
    let summary = inner
        .active_summary
        .lock()
        .map_err(|_| SdkError::Internal("active_summary 锁已损坏".to_string()))?
        .clone();
    let frozen_chats = inner
        .frozen_chats
        .lock()
        .map_err(|_| SdkError::Internal("frozen_chats 锁已损坏".to_string()))?
        .clone();

    // 构造 chats = 旧链（冻结）+ 活跃段
    let mut chats = frozen_chats;
    if let Some(s) = summary {
        chats.push(crate::business::session::ChatSegment::compact(s, messages));
    } else {
        let mut seg = crate::business::session::ChatSegment::normal(None);
        seg.messages = messages;
        chats.push(seg);
    }

    let mut session = crate::business::session::Session::new(
        inner.session_id.clone(),
        inner.cwd.to_string_lossy().to_string(),
    );
    session.chats = chats;
    // 旧 messages 字段置空（已迁移到 chats）
    session.messages = Vec::new();
    session.updated_at = crate::business::session::now_iso();
    session.metadata.model = Some(mapping::model_display(
        &inner.resolved_model.source_key,
        &inner.resolved_model.model.name,
        &inner.resolved_model.model.id,
    ));
    session.tasks = task_snapshot;
    session.workspace = workspace;
    crate::business::session::save_session(&session)
        .await
        .map_err(SdkError::Session)
}

pub(super) async fn list_sessions_impl(
    _me: &AgentClientImpl,
) -> Result<Vec<SessionSummary>, SdkError> {
    Ok(crate::business::session::list_sessions()
        .await
        .into_iter()
        .map(mapping::session_summary_from_runtime)
        .collect())
}
