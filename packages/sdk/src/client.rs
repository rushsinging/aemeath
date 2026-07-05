//! AgentClient trait — Agent Runtime 对外的统一接口。

use async_trait::async_trait;

use crate::{ChatMessage, ChatRequest, ChatStream, ModelSummary, SessionSnapshot, SessionSummary};

/// Agent Runtime 的统一客户端 trait。
///
/// #567 后 trait 正在收窄为只有 `chat()`——所有交互通过事件流。
/// 下方方法为启动时（loop 未运行）的一次性查询，后续 S12 拆到独立接口。
#[async_trait]
pub trait AgentClient: Send + Sync + 'static {
    /// 发起一次 Chat，返回事件流。
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, super::SdkError>;

    // ─── 启动时一次性查询（loop 未运行，无法走事件流） ───

    async fn chat_text(&self, input: crate::ChatInput) -> Result<ChatStream, super::SdkError> {
        self.chat(ChatRequest {
            messages: vec![ChatMessage::user_text(input.text)],
            queue_drain: None,
            input_events: None,
        })
        .await
    }

    async fn load_session(&self, id: &str) -> Result<SessionSnapshot, super::SdkError>;

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, super::SdkError>;

    async fn delete_session(&self, id: &str) -> Result<(), super::SdkError>;

    async fn list_models(&self) -> Result<Vec<ModelSummary>, super::SdkError>;

    // ─── 临时保留（run_loop 轮询机制，后续 PR 改为事件驱动） ───

    fn changes(&self) -> tokio::sync::watch::Receiver<crate::ChangeSet> {
        unimplemented!("#567: changes 待迁移到事件驱动")
    }
}
