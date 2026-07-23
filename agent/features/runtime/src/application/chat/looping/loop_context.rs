//! chat loop 的上下文类型定义。
//!
//! `SwitchClientFn` 和 `ChatLoopContext` 从 `loop_runner.rs` 拆出，
//! 降低主循环文件的体量。

use crate::application::chat::looping::events::ChatEventSink;
use crate::application::chat::looping::input_gate::InputEventDrainPort;
use crate::application::chat::looping::queue::QueueDrainPort;
use std::sync::Arc;
use tools::{ToolCatalogPort, ToolExecutionPort};
use workflow::api::ReasoningPort;

/// 模型切换构建器类型（#567）：接受 selection 字符串，async 返回
/// `(ProviderBinding, ModelSwitchResult)` 或 `String` 错误。
pub type SwitchClientFn = Arc<
    dyn Fn(
            &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<
                            (crate::ports::ProviderBinding, sdk::ModelSwitchResult),
                            String,
                        >,
                    > + Send,
            >,
        > + Send
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
    pub binding: Arc<crate::ports::ProviderBinding>,
    pub tool_catalog: Arc<dyn ToolCatalogPort>,
    pub tool_execution: Arc<dyn ToolExecutionPort>,
    pub tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    pub system_blocks: Vec<provider::RequestSystemBlock>,
    pub system_prompt_text: String,
    /// 只在本 ChatLoop 启动时投递一次的 Git 上下文普通消息。
    pub initial_git_context: String,
    pub user_context: String,
    /// 本轮 chat loop 的初始消息（来自 user_input）。Runtime 不再持有/回写
    /// 会话链；历史由 Context backing 提供。
    pub initial_messages: Vec<share::message::Message>,
    pub context_size: usize,
    pub workspace: project::WorkspaceViews,
    /// Context-owned Main Session coordinator. Used for:
    /// - `bind_main_run` before each Run (admission gate)
    /// - `resume_session_to_backing` for runtime ResumeSession commands
    pub wiring: Arc<context::MainSessionWiring>,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<tools::SessionReminders>>,
    pub agent_runner: Option<Arc<dyn tools::AgentRunner>>,
    pub tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    pub policy: Arc<dyn policy::PolicyPort>,
    pub(crate) active_run: Arc<dyn crate::domain::agent_run::ActiveRunPort>,
    /// #1246: Typed interaction bridge for AskUserQuestion suspension.
    pub interaction_bridge: Arc<crate::application::interaction::InteractionBridge>,
    /// Runtime/Tool 日常状态唯一来源（#889 low-privilege 端口）。
    pub task_access: Arc<dyn task::TaskAccess>,
    pub max_tool_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: std::sync::Arc<dyn hook::HookPort>,
    pub memory_config: share::config::MemoryConfig,
    /// Memory domain port（MemoryTool 使用）。
    pub memory: Arc<dyn memory::MemoryPort>,
    /// Memory-owned reflection history write boundary used by background tasks.
    pub reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
    pub language: String,
    /// Workflow-owned Main adaptive reasoning capability。
    pub reasoning: Arc<dyn ReasoningPort>,
    /// 模型切换构建器（#567）。由 core 层注入，避免 business 层反向依赖 core。
    /// idle 分支收到 `SwitchModel` 事件时调用，从 config 解析 selection 字符串，
    /// 返回新 `ProviderBinding` + `ModelSwitchResult`；解析失败返回 `String` 错误信息。
    pub build_switched_client: SwitchClientFn,
    /// 查询 reflection 历史（#899）。返回安全视图，不含正文。
    pub list_reflection_history: Arc<
        dyn Fn(
                usize,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<Vec<sdk::ReflectionHistoryView>, sdk::SdkError>,
                        > + Send,
                >,
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
