//! chat loop 的上下文类型定义。
//!
//! `SwitchClientFn` 和 `ChatLoopContext` 从 `loop_runner.rs` 拆出，
//! 降低主循环文件的体量。

use crate::application::chat::looping::events::ChatEventSink;
use crate::application::chat::looping::input_gate::InputEventDrainPort;
use crate::application::chat::looping::queue::QueueDrainPort;
use std::sync::Arc;
use tools::api::ToolRegistry;
use workflow::api::ReasoningPort;

/// 模型切换构建器类型（#567）：接受 selection 字符串，async 返回
/// `(LlmClient, ModelSwitchResult)` 或 `String` 错误。
pub type SwitchClientFn = Arc<
    dyn Fn(
            &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<
                            (provider::LlmClient, sdk::ModelSwitchResult),
                            String,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

pub type SaveChainFn = Arc<
    dyn Fn(
            &context::session::ChatChain,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), sdk::SdkError>> + Send>>
        + Send
        + Sync,
>;

/// 单次 chat loop 的完整执行状态。
///
/// 由 `chat_impl()` 从 `RuntimeHandle` 构造，按值传入 `process_chat_loop()`，
/// 函数内解构消费。持有 session 级不变配置 + loop 专属可变状态（messages、cancel 等）。
#[allow(clippy::type_complexity)]
pub struct ChatLoopContext<S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    pub sink: S,
    pub queue: Q,
    pub input_events: I,
    pub client: Arc<provider::LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<provider::SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub chain: context::session::ChatChain,
    pub context_size: usize,
    pub workspace: project::WorkspaceViews,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<share::tool::SessionReminders>>,
    pub agent_runner: Option<Arc<dyn tools::api::AgentRunner>>,
    pub allow_all: bool,
    pub(crate) active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    /// Legacy 持久化兼容句柄（input_gate clear，#890/#891）。
    pub task_store: Arc<storage::TaskStore>,
    /// Runtime/Tool 日常状态唯一来源（#889 low-privilege 端口）。
    pub task_access: Arc<dyn task::TaskAccess>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: hook::api::HookRunner,
    pub memory_config: share::config::MemoryConfig,
    pub language: String,
    /// Workflow-owned Main adaptive reasoning capability。
    pub reasoning: Arc<dyn ReasoningPort>,
    /// Compact 时冻结的旧链（保留在 session 文件中供审计，resume 不加载）。
    pub frozen_chats: Arc<std::sync::Mutex<Vec<context::session::ChatSegment>>>,
    /// 活跃链的 compact summary（走 system 通道注入）。
    pub active_summary: Arc<std::sync::Mutex<Option<String>>>,
    /// 模型切换构建器（#567）。由 core 层注入，避免 business 层反向依赖 core。
    /// idle 分支收到 `SwitchModel` 事件时调用，从 config 解析 selection 字符串，
    /// 返回新 `LlmClient` + `ModelSwitchResult`；解析失败返回 `String` 错误信息。
    pub build_switched_client: SwitchClientFn,
    /// 会话保存闭包（#688）。由 core 层注入，直接接受 chain 引用保存。
    pub save_chain: SaveChainFn,
    /// 运行 reflection（#567）。由 core 层注入。
    pub run_reflection_on_demand: Arc<
        dyn Fn() -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<sdk::ReflectionOutputView, sdk::SdkError>,
                        > + Send,
                >,
            > + Send
            + Sync,
    >,
    /// 应用 reflection 结果（#567）。由 core 层注入。
    pub apply_reflection_on_demand: Arc<
        dyn Fn(
                sdk::ReflectionOutputView,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<String, sdk::SdkError>> + Send>,
            > + Send
            + Sync,
    >,
    /// 查询模型列表（#567）。由 core 层注入。
    pub list_models: Arc<
        dyn Fn() -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<Vec<sdk::ModelSummary>, sdk::SdkError>>
                        + Send,
                >,
            > + Send
            + Sync,
    >,
    /// 查询提醒列表（#567）。由 core 层注入。
    pub list_reminders: Arc<
        dyn Fn() -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<Vec<sdk::ReminderView>, sdk::SdkError>>
                        + Send,
                >,
            > + Send
            + Sync,
    >,
    /// 查询会话列表（#567 S14）。由 core 层注入。
    pub list_sessions: Arc<
        dyn Fn() -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<Vec<sdk::SessionSummary>, sdk::SdkError>,
                        > + Send,
                >,
            > + Send
            + Sync,
    >,
}
