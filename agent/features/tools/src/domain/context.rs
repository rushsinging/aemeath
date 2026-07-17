use super::resources::ToolResources;
use crate::domain::{AgentProgressEvent, SessionReminders};
use project::{WorkspaceControl, WorkspaceRead, WorkspaceViews};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// 单次 tool call 的执行环境。
///
/// 由 runtime 的 `process_chat_loop` 每回合构造，按引用传入各 `Tool::call()`。
/// 持有 [`ToolResources`](super::resources::ToolResources)（session 级不变共享件）
/// + tool 执行专属的可变状态（cancel、read_files 等）。
#[derive(Clone)]
pub struct ToolExecutionContext {
    /// 共享资源（agent_runner / registry / memory_config / lang / allow_all）。
    pub resources: ToolResources,

    /// Project-owned 窄 workspace views；不暴露具体 service。
    pub workspace: WorkspaceViews,
    /// Runtime Run ID 的 Published Language 字符串；Tools 只透传，不解释领域身份。
    pub run_id: String,
    pub cancel: CancellationToken,
    pub read_files: Arc<Mutex<HashSet<String>>>,
    /// Session-local reminders shared by MemoryTool and UI/REPL.
    pub session_reminders: Option<Arc<Mutex<SessionReminders>>>,
    /// Whether we're in plan mode (simulated tool execution)
    pub plan_mode: Option<bool>,
    /// Maximum number of concurrent tool executions (from tools.maxConcurrency)
    pub max_tool_concurrency: usize,
    /// Maximum number of concurrent sub-agent executions (from agents.maxConcurrency)
    pub max_agent_concurrency: usize,
    /// Semaphore to limit concurrent sub-agent executions (shared across tool calls)
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    /// Channel to send agent progress updates to the TUI (tool_id → progress event).
    /// Populated when an Agent tool call is in flight, so CliAgentRunner can stream
    /// per-turn structured output back to the user.
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    /// Parent chat session id. Used by sub-agent/tool logs to correlate activity
    /// back to the user-visible session.
    pub parent_session_id: Option<String>,
}

impl ToolExecutionContext {
    /// 只读 workspace 能力（所有 tool）。
    pub fn workspace_read(&self) -> Arc<dyn WorkspaceRead> {
        self.workspace.read()
    }
    /// 变更 workspace 能力（仅 bash + worktree 工具；由 guard 约束调用方）。
    pub fn workspace_control(&self) -> Arc<dyn WorkspaceControl> {
        self.workspace.control()
    }

    pub fn derive_isolated_workspace(&self) -> WorkspaceViews {
        self.workspace.derive_isolated()
    }
}
