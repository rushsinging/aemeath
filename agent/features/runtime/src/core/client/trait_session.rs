//! session 相关方法实际逻辑。

use std::sync::Arc;

use sdk::{SdkError, SessionSnapshot, SessionSummary};

use super::accessors::{AgentClientImpl, RuntimeHandle};
use super::mapping;

pub(super) async fn sync_current_messages_impl(
    me: &AgentClientImpl,
    messages: Vec<sdk::ChatMessage>,
) -> Result<(), SdkError> {
    *me.inner
        .current_messages
        .lock()
        .map_err(|_| SdkError::Internal("当前 session 消息锁已损坏".to_string()))? = messages
        .into_iter()
        .map(mapping::message_from_sdk)
        .collect();
    Ok(())
}

pub(super) async fn save_current_session_impl(me: &AgentClientImpl) -> Result<(), SdkError> {
    save_session_from_handle(&me.inner).await
}

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

pub(super) async fn load_session_impl(
    me: &AgentClientImpl,
    id: &str,
) -> Result<SessionSnapshot, SdkError> {
    match crate::business::session::load_session(id).await {
        Ok(session) => {
            // 统一通过 SessionRestore 提取活跃链运行时状态（与 loop_runner::ResumeSession 共享）
            let restore = crate::business::session::SessionRestore::from_session(&session);

            // 写回 RuntimeHandle 状态
            if let Ok(mut guard) = me.inner.active_summary.lock() {
                *guard = restore.active_summary;
            }
            if let Ok(mut guard) = me.inner.frozen_chats.lock() {
                *guard = restore.frozen_chats;
            }

            let sdk_messages: Vec<sdk::ChatMessage> = restore
                .active_messages
                .into_iter()
                .map(mapping::message_to_sdk)
                .collect();
            let count = sdk_messages.len();
            let total_tokens: u64 = sdk_messages
                .iter()
                .map(|m| {
                    let text = m.text_content();
                    text.len() as u64 / 4
                })
                .sum();
            let workspace_sdk = session
                .workspace
                .as_ref()
                .map(|ws| mapping::workspace_context_to_sdk(ws.clone()));
            // 恢复 runtime handle 的 workspace 服务状态，使后续 chat() 调用使用正确的 worktree 路径
            if let Some(ref ws) = session.workspace {
                let _ = project::api::WorkspacePersist::restore(me.inner.workspace.as_ref(), ws);
            }
            Ok(SessionSnapshot {
                id: session.id,
                message_count: count,
                total_tokens,
                messages: sdk_messages,
                created_at: Some(restore.created_at),
                trimmed: restore.trimmed,
                repaired: restore.repaired,
                workspace: workspace_sdk,
                tasks: session
                    .tasks
                    .map(|t| serde_json::to_value(t).unwrap_or_default()),
            })
        }
        Err(e) => match e {
            crate::business::session::SessionLoadError::NotFound { id } => {
                Err(SdkError::SessionNotFound { id })
            }
            crate::business::session::SessionLoadError::Corrupt {
                id,
                parse_err,
                corrupt_path,
            } => Err(SdkError::SessionCorrupt {
                id,
                parse_err,
                corrupt_path: corrupt_path.to_string_lossy().to_string(),
            }),
            crate::business::session::SessionLoadError::Io { id, source } => Err(
                SdkError::Session(format!("Failed to read session {id}: {source}")),
            ),
        },
    }
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

pub(super) async fn delete_session_impl(_me: &AgentClientImpl, id: &str) -> Result<(), SdkError> {
    crate::business::session::delete_session(id)
        .await
        .map_err(SdkError::Session)
}
