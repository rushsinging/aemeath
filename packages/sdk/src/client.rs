//! AgentClient trait — Agent Runtime 对外的统一接口。

use async_trait::async_trait;

use crate::{
    ChatMessage, ChatRequest, ChatStream, ClipboardImageView, CostInfo, ModelSummary,
    ProjectContext, ReflectionOutputView, SessionSnapshot, SessionSummary, TaskStatusView,
};

/// Agent Runtime 的统一客户端 trait。
///
/// #567 后 trait 正在收窄为只有 `chat()`——所有交互通过事件流。
/// 下方 `#[deprecated]` 方法是 TUI 侧尚未迁移的临时 stub，后续 PR 逐个删除。
#[async_trait]
pub trait AgentClient: Send + Sync + 'static {
    /// 发起一次 Chat，返回事件流。
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, super::SdkError>;

    // ─── 以下方法为 #567 迁移期间的临时 stub，后续 PR 逐个删除 ───

    async fn chat_text(&self, input: crate::ChatInput) -> Result<ChatStream, super::SdkError> {
        self.chat(ChatRequest {
            messages: vec![ChatMessage::user_text(input.text)],
            queue_drain: None,
            input_events: None,
        })
        .await
    }

    fn session_snapshot(&self) -> SessionSnapshot {
        unimplemented!("#567: session_snapshot 待迁移")
    }
    fn cost(&self) -> CostInfo {
        unimplemented!("#567: cost 待迁移")
    }
    async fn task_status(&self) -> Result<TaskStatusView, super::SdkError> {
        unimplemented!("#567: task_status 待迁移")
    }
    fn project(&self) -> ProjectContext {
        unimplemented!("#567: project 待迁移")
    }
    fn changes(&self) -> tokio::sync::watch::Receiver<crate::ChangeSet> {
        unimplemented!("#567: changes 待迁移")
    }
    async fn sync_current_messages(
        &self,
        _messages: Vec<ChatMessage>,
    ) -> Result<(), super::SdkError> {
        unimplemented!("#567: sync_current_messages 待迁移")
    }
    async fn save_current_session(&self) -> Result<(), super::SdkError> {
        unimplemented!("#567: save_current_session 待迁移")
    }
    fn set_current_turn(&self, _turn: usize) {}
    async fn load_session(&self, _id: &str) -> Result<SessionSnapshot, super::SdkError> {
        unimplemented!("#567: load_session 待迁移")
    }
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, super::SdkError> {
        unimplemented!("#567: list_sessions 待迁移")
    }
    async fn delete_session(&self, _id: &str) -> Result<(), super::SdkError> {
        unimplemented!("#567: delete_session 待迁移")
    }
    async fn list_models(&self) -> Result<Vec<ModelSummary>, super::SdkError> {
        unimplemented!("#567: list_models 待迁移")
    }
    async fn read_clipboard_image(&self) -> Result<ClipboardImageView, super::SdkError> {
        unimplemented!("#567: read_clipboard_image 待迁移")
    }
    async fn process_image_file(
        &self,
        _path: String,
    ) -> Result<ClipboardImageView, super::SdkError> {
        unimplemented!("#567: process_image_file 待迁移")
    }
    async fn run_reflection(
        &self,
        _messages: Vec<ChatMessage>,
    ) -> Result<ReflectionOutputView, super::SdkError> {
        unimplemented!("#567: run_reflection 待迁移")
    }
    async fn apply_reflection(
        &self,
        _output: ReflectionOutputView,
    ) -> Result<String, super::SdkError> {
        unimplemented!("#567: apply_reflection 待迁移")
    }
    async fn notify_hook(&self, _message: &str, _kind: &str) -> Result<(), super::SdkError> {
        unimplemented!("#567: notify_hook 待迁移")
    }
    async fn list_reminders(&self) -> Result<Vec<crate::ReminderView>, super::SdkError> {
        unimplemented!("#567: list_reminders 待迁移")
    }
    async fn restore_tasks(&self, _snapshot: serde_json::Value) -> Result<(), super::SdkError> {
        unimplemented!("#567: restore_tasks 待迁移")
    }
}
