use super::AgentRunner;
use project::api::{WorkspaceControl, WorkspaceRead, WorkspaceService};
use share::tool::{AgentProgressEvent, SessionReminders};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ToolExecutionContext {
    /// Initial workspace root, kept for compatibility with existing callers.
    pub cwd: PathBuf,
    /// 唯一 workspace 状态源句柄（project 拥有）。
    pub workspace: Arc<WorkspaceService>,
    pub cancel: CancellationToken,
    pub read_files: Arc<Mutex<HashSet<String>>>,
    pub agent_runner: Option<Arc<dyn AgentRunner>>,
    /// Session-local reminders shared by MemoryTool and UI/REPL.
    pub session_reminders: Option<Arc<Mutex<SessionReminders>>>,
    /// Memory system configuration used by MemoryTool.
    pub memory_config: share::config::MemoryConfig,
    /// Whether we're in plan mode (simulated tool execution)
    pub plan_mode: Option<bool>,
    /// Whether all tools are auto-approved (skip injection checks)
    pub allow_all: bool,
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
    pub fn workspace_read(&self) -> &dyn WorkspaceRead {
        self.workspace.as_ref()
    }
    /// 变更 workspace 能力（仅 bash + worktree 工具；由 guard 约束调用方）。
    pub fn workspace_control(&self) -> &dyn WorkspaceControl {
        self.workspace.as_ref()
    }
}
