//! TUI 启动投影。
//!
//! 该模块仍包含 runtime 内部类型，因为当前 TUI 尚未完全 SDK 化；它把
//! `AgentClientImpl` 暴露给 CLI 的访问集中到单一过渡结构，避免 CLI
//! composition root 继续散落读取 runtime 内部字段。

use std::sync::Arc;

use hook::HookPort;
use provider::RequestSystemBlock;
use tools::{AgentRunner, ToolCatalogPort, ToolExecutionPort};

use crate::ports::ProviderBinding;

/// TUI 启动所需的过渡上下文。
pub struct TuiLaunchContext {
    pub session_id: String,
    pub model_display: String,
    pub binding: Arc<ProviderBinding>,
    pub tool_catalog: Arc<dyn ToolCatalogPort>,
    pub tool_execution: Arc<dyn ToolExecutionPort>,
    pub system_blocks: Vec<RequestSystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub context_size: usize,
    pub verbose: bool,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub allow_all: bool,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub memory_config: sdk::MemoryConfigView,
    pub skills_map: std::collections::HashMap<String, sdk::SkillView>,
    pub hook_runner: Arc<dyn HookPort>,
    /// 本地 session reminders（用于 TUI 展示，独立于 RuntimeHandle 实例）
    pub session_reminders: Arc<std::sync::Mutex<tools::SessionReminders>>,
    /// #567：项目工作区根路径（替代 client.project() RPC）
    pub workspace_root: std::path::PathBuf,
}
