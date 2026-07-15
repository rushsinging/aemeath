//! session 相关方法实际逻辑。

use std::sync::Arc;

use sdk::{SdkError, SessionSummary};

use super::accessors::{AgentClientImpl, RuntimeHandle};
use super::mapping;

/// 从 chain + handle 数据保存 session。
/// loop 内部直接传入 chain（无需经过 inner.current_chain）。
pub(super) async fn save_chain_to_handle(
    chain: &context::session::ChatChain,
    inner: &Arc<RuntimeHandle>,
) -> Result<(), SdkError> {
    // 从 chain 取活跃段（真实 segment 边界）
    let active_segments: Vec<context::session::ChatSegment> = chain.active_segments().to_vec();
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
