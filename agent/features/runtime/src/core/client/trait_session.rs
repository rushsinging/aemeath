//! session 相关方法实际逻辑。

use sdk::{SdkError, SessionSnapshot, SessionSummary};

use super::accessors::AgentClientImpl;
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
    let messages = me
        .inner
        .current_messages
        .lock()
        .map_err(|_| SdkError::Internal("当前 session 消息锁已损坏".to_string()))?
        .clone();
    let task_snapshot = {
        let snap = me.inner.context.task_store.snapshot().await;
        if snap.tasks.is_empty() {
            None
        } else {
            Some(snap)
        }
    };
    let workspace = me
        .inner
        .workspace_context
        .lock()
        .map_err(|_| SdkError::Internal("当前工作区上下文锁已损坏".to_string()))?
        .clone();
    let mut session = crate::business::session::Session::new(
        me.inner.session_id.clone(),
        me.inner.cwd.to_string_lossy().to_string(),
    );
    session.messages = messages;
    session.updated_at = crate::business::session::now_iso();
    session.metadata.model = Some(mapping::model_display(
        &me.inner.resolved_model.source_key,
        &me.inner.resolved_model.model.name,
        &me.inner.resolved_model.model.id,
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
            let mut messages = session.messages;
            let trimmed = {
                let before = messages.len();
                crate::business::chat::message_integrity::sanitize_messages(&mut messages);
                before.saturating_sub(messages.len())
            };
            let repaired = {
                let integrity =
                    crate::business::chat::message_integrity::check_message_integrity(&messages);
                if integrity.has_issues() {
                    crate::business::chat::message_integrity::deep_clean_messages(&mut messages)
                } else {
                    0
                }
            };
            let sdk_messages: Vec<sdk::ChatMessage> =
                messages.into_iter().map(mapping::message_to_sdk).collect();
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
            // 恢复 runtime handle 的 workspace_context，使后续 chat() 调用使用正确的 worktree 路径
            if let Some(ref ws) = session.workspace {
                if let Ok(mut guard) = me.inner.workspace_context.lock() {
                    *guard = Some(ws.clone());
                }
            }
            Ok(SessionSnapshot {
                id: session.id,
                message_count: count,
                total_tokens,
                messages: sdk_messages,
                created_at: Some(session.created_at),
                trimmed,
                repaired,
                workspace: workspace_sdk,
                tasks: session
                    .tasks
                    .map(|t| serde_json::to_value(t).unwrap_or_default()),
            })
        }
        Err(e) => Err(SdkError::Internal(format!(
            "Failed to load session {id}: {e}"
        ))),
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
