//! AgentClient trait — Agent Runtime 对外的统一接口。

use async_trait::async_trait;

use crate::{
    ChangeSet, ChatInput, ChatStream, CostInfo, ModelSummary, ProjectContext, SessionSnapshot,
    TaskSummary,
};

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

    /// 获取当前项目上下文（Copy 值类型）。
    fn project(&self) -> ProjectContext;

    // ─── 变更通道 ───

    /// 订阅变更通道，TUI 通过 `changed()` 检测哪些领域发生了变化。
    fn changes(&self) -> tokio::sync::watch::Receiver<ChangeSet>;

    // ─── 写操作 ───

    /// 发起一次 Chat。
    async fn chat(&self, input: ChatInput) -> Result<ChatStream, super::SdkError>;

    /// 取消当前进行中的 Chat。
    fn cancel(&self);

    /// 保存当前 session。
    async fn save_session(&self) -> Result<(), super::SdkError>;

    /// 加载指定 session。
    async fn load_session(&self, id: &str) -> Result<SessionSnapshot, super::SdkError>;

    /// 列出所有 session 摘要。
    async fn list_sessions(&self) -> Result<Vec<super::session::SessionSummary>, super::SdkError>;

    /// 删除指定 session。
    async fn delete_session(&self, id: &str) -> Result<(), super::SdkError>;

    /// 列出可用模型摘要。
    async fn list_models(&self) -> Result<Vec<ModelSummary>, super::SdkError>;

    /// 压缩 session 消息。
    async fn compact(&self) -> Result<(), super::SdkError>;
}
