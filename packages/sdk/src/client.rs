//! AgentClient trait — Agent Runtime 对外的统一接口。

use async_trait::async_trait;

use crate::{
    ChangeSet, ChatInput, ChatRequest, ChatStream, ClipboardImageView, CostInfo, ModelSummary,
    ProjectContext, ReflectionOutputView, SessionSnapshot, TaskStatusView, TaskSummary,
};

use crate::chat_event::ChatEvent;

/// Agent Runtime 的统一客户端 trait。
///
/// CLI（薄入口）通过此 trait 与 Runtime 通信，不直接依赖 Runtime 内部类型。
#[async_trait]
pub trait AgentClient: Send + Sync + 'static {
    // ─── 快照（无锁，永不阻塞） ───

    /// 获取当前 session 快照（cheap clone）。
    fn session_snapshot(&self) -> SessionSnapshot;

    /// 获取当前成本信息（Atomic 读取，纳秒级）。
    fn cost(&self) -> CostInfo;

    /// 获取当前任务列表快照。
    fn task_list(&self) -> Vec<TaskSummary>;

    /// 获取 TUI 可展示的任务状态视图。
    async fn task_status(&self) -> Result<TaskStatusView, super::SdkError>;

    /// 获取当前项目上下文（Copy 值类型）。
    fn project(&self) -> ProjectContext;

    // ─── 变更通道 ───

    /// 订阅变更通道，TUI 通过 `changed()` 检测哪些领域发生了变化。
    fn changes(&self) -> tokio::sync::watch::Receiver<ChangeSet>;

    // ─── 写操作 ───

    /// 发起一次 Chat。
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, super::SdkError>;

    /// 兼容非 TUI 调用方的一次性文本 Chat。
    async fn chat_text(&self, input: ChatInput) -> Result<ChatStream, super::SdkError> {
        self.chat(ChatRequest {
            messages: vec![super::ChatMessage::user_text(input.text)],
            queue_drain: None,
            input_events: None,
        })
        .await
    }

    /// 更新 runtime 持有的当前 session messages。
    async fn sync_current_messages(
        &self,
        _messages: Vec<super::ChatMessage>,
    ) -> Result<(), super::SdkError> {
        Ok(())
    }

    /// 保存 runtime 当前 session。
    async fn save_current_session(&self) -> Result<(), super::SdkError>;

    /// 取消当前进行中的 Chat。
    fn cancel(&self);

    /// 设置当前 turn 编号（由 TUI run_loop 在每次新请求时调用）。
    fn set_current_turn(&self, _turn: usize) {}

    /// 加载指定 session。
    async fn load_session(&self, id: &str) -> Result<SessionSnapshot, super::SdkError>;

    /// 列出所有 session 摘要。
    async fn list_sessions(&self) -> Result<Vec<super::session::SessionSummary>, super::SdkError>;

    /// 删除指定 session。
    async fn delete_session(&self, id: &str) -> Result<(), super::SdkError>;

    /// 列出可用模型摘要。
    async fn list_models(&self) -> Result<Vec<ModelSummary>, super::SdkError>;

    /// 读取剪贴板图片，返回 TUI 可渲染视图。
    async fn read_clipboard_image(&self) -> Result<ClipboardImageView, super::SdkError>;

    /// 处理图片文件，返回 TUI 可渲染视图。
    async fn process_image_file(&self, path: String)
        -> Result<ClipboardImageView, super::SdkError>;

    /// 基于当前消息运行 reflection。
    async fn run_reflection(
        &self,
        messages: Vec<super::ChatMessage>,
    ) -> Result<ReflectionOutputView, super::SdkError>;

    /// 应用 reflection 结果到记忆系统。
    async fn apply_reflection(
        &self,
        output: ReflectionOutputView,
    ) -> Result<String, super::SdkError>;

    /// 设置推理模式（None = 切换）。
    async fn set_thinking(&self, desired: Option<bool>) -> Result<bool, super::SdkError>;

    // ─── Hook ───

    /// 触发 hook 通知（消息变更等）。
    async fn notify_hook(&self, message: &str, kind: &str) -> Result<(), super::SdkError>;

    // ─── Reminder ───

    /// 列出当前 session 的 reminders。
    async fn list_reminders(&self) -> Result<Vec<super::ReminderView>, super::SdkError>;

    /// 添加 reminder。
    async fn add_reminder(&self, content: &str) -> Result<String, super::SdkError>;

    /// 完成指定 reminder。
    async fn complete_reminder(&self, id: &str) -> Result<(), super::SdkError>;

    // ─── Thinking ───

    /// 获取当前推理模式状态。
    async fn get_thinking(&self) -> Result<bool, super::SdkError>;

    // ─── TaskStore ───

    /// 恢复 TaskStore 快照。
    async fn restore_tasks(&self, snapshot: serde_json::Value) -> Result<(), super::SdkError>;

    /// 清空 Runtime 持有的 TaskStore。
    async fn clear_tasks(&self) -> Result<(), super::SdkError>;
}
