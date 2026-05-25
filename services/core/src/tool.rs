use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// 保存进入 worktree 前的工作上下文快照
#[derive(Debug, Clone)]
pub struct WorkingContext {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ImageData {
    pub base64: String,
    pub media_type: String,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
    /// Optional images to include in the tool result (for vision-capable models)
    pub images: Vec<ImageData>,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
            images: Vec::new(),
        }
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
            images: Vec::new(),
        }
    }

    pub fn with_image(mut self, base64: String, media_type: String) -> Self {
        self.images.push(ImageData { base64, media_type });
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentProgressEvent {
    /// Monotonic sequence for internal ordering/replacement. UI does not display it by default.
    pub sequence: usize,
    pub kind: AgentProgressKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentProgressKind {
    ToolCalls { calls: Vec<AgentToolCallProgress> },
    Message { text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolCallProgress {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub summary: String,
}

/// Callback for running a sub-agent loop. Implemented by the CLI layer.
#[async_trait]
pub trait AgentRunner: Send + Sync {
    async fn run_agent(
        &self,
        prompt: &str,
        system: &str,
        tool_schemas: &[serde_json::Value],
        registry: &ToolRegistry,
        ctx: &ToolContext,
        max_turns: Option<u32>,
        model_spec: Option<&str>,
        // Optional channel to stream per-turn progress to TUI
        progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    ) -> String;

    /// Single-turn LLM completion (no tool loop). Used for analysis/planning.
    async fn complete(&self, prompt: &str, system: &str, ctx: &ToolContext) -> String;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SessionReminder {
    pub id: String,
    pub content: String,
    pub done: bool,
    pub created_at: u64,
}

#[derive(Debug, Default, Clone)]
pub struct SessionReminders {
    reminders: Vec<SessionReminder>,
}

impl SessionReminders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, content: impl Into<String>) -> Result<String, String> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err("reminder 内容不能为空".to_string());
        }

        let id = uuid::Uuid::now_v7().to_string();
        self.reminders.push(SessionReminder {
            id: id.clone(),
            content,
            done: false,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
        });
        Ok(id)
    }

    pub fn complete(&mut self, id: &str) -> Result<(), String> {
        let reminder = self
            .reminders
            .iter_mut()
            .find(|reminder| reminder.id == id)
            .ok_or_else(|| format!("memory not found: {id}"))?;
        reminder.done = true;
        Ok(())
    }

    pub fn list(&self) -> &[SessionReminder] {
        &self.reminders
    }

    pub fn clear(&mut self) {
        self.reminders.clear();
    }

    pub fn recap_line(&self) -> Option<String> {
        let active = self
            .reminders
            .iter()
            .filter(|reminder| !reminder.done)
            .map(|reminder| reminder.content.as_str())
            .collect::<Vec<_>>();

        if active.is_empty() {
            None
        } else {
            Some(format!("* recap: {}", active.join(" | ")))
        }
    }
}

#[derive(Clone)]
pub struct ToolContext {
    /// Initial workspace root, kept for compatibility with existing callers.
    pub cwd: PathBuf,
    /// Active workspace root used as the security boundary for file/search tools.
    pub working_root: Arc<Mutex<PathBuf>>,
    /// Base directory used to resolve relative file/tool paths.
    pub path_base: Arc<Mutex<PathBuf>>,
    pub cancel: CancellationToken,
    pub read_files: std::sync::Arc<Mutex<HashSet<String>>>,
    pub agent_runner: Option<std::sync::Arc<dyn AgentRunner>>,
    /// Session-local reminders shared by MemoryTool and UI/REPL.
    pub session_reminders: Option<Arc<Mutex<SessionReminders>>>,
    /// Memory system configuration used by MemoryTool.
    pub memory_config: crate::config::MemoryConfig,
    /// Whether we're in plan mode (simulated tool execution)
    pub plan_mode: Option<bool>,
    /// Whether all tools are auto-approved (skip injection checks)
    pub allow_all: bool,
    /// Maximum number of concurrent tool executions (from tools.maxConcurrency)
    pub max_tool_concurrency: usize,
    /// Maximum number of concurrent sub-agent executions (from agents.maxConcurrency)
    pub max_agent_concurrency: usize,
    /// Semaphore to limit concurrent sub-agent executions (shared across tool calls)
    pub agent_semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    /// Channel to send agent progress updates to the TUI (tool_id → progress event).
    /// Populated when an Agent tool call is in flight, so CliAgentRunner can stream
    /// per-turn structured output back to the user.
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    /// Parent chat session id. Used by sub-agent/tool logs to correlate activity
    /// back to the user-visible session.
    pub parent_session_id: Option<String>,
    /// 上下文栈：EnterWorktree push，ExitWorktree pop
    pub context_stack: Arc<Mutex<Vec<WorkingContext>>>,
}

impl ToolContext {
    pub fn new_working_paths(cwd: PathBuf) -> (PathBuf, Arc<Mutex<PathBuf>>, Arc<Mutex<PathBuf>>) {
        let working_root = Arc::new(Mutex::new(cwd.clone()));
        let path_base = Arc::new(Mutex::new(cwd.clone()));
        (cwd, working_root, path_base)
    }

    pub fn current_working_root(&self) -> PathBuf {
        self.working_root
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn current_path_base(&self) -> PathBuf {
        self.path_base
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn set_working_directory(&self, path: PathBuf) {
        let working_root = detect_working_root(&path);
        match self.working_root.lock() {
            Ok(mut current) => *current = working_root,
            Err(poisoned) => *poisoned.into_inner() = working_root,
        }
        match self.path_base.lock() {
            Ok(mut path_base) => *path_base = path,
            Err(poisoned) => *poisoned.into_inner() = path,
        }
    }
}

fn detect_working_root(path: &std::path::Path) -> PathBuf {
    std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| PathBuf::from(stdout.trim()))
        .filter(|root| !root.as_os_str().is_empty())
        .unwrap_or_else(|| path.to_path_buf())
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    /// Timeout for this tool in seconds (default 120s, override for long-running tools)
    fn timeout_secs(&self) -> u64 {
        120
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult;
}

pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, tool: Box<dyn Tool>) {
        self.tools
            .write()
            .insert(tool.name().to_string(), Arc::from(tool));
    }

    pub fn unregister(&self, name: &str) -> bool {
        self.tools.write().remove(name).is_some()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.read().contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.tools.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.read().is_empty()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().get(name).cloned()
    }

    pub fn schemas(&self) -> Vec<Value> {
        self.tools
            .read()
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.input_schema(),
                })
            })
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.read().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tool_registry_tests {
    use super::*;

    struct DummyTool {
        name: String,
        description: String,
    }

    impl DummyTool {
        fn new(name: &str, description: &str) -> Self {
            Self {
                name: name.to_string(),
                description: description.to_string(),
            }
        }
    }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }

        async fn call(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::success("ok")
        }
    }

    #[test]
    fn test_tool_registry_unregister_existing_tool() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool::new("dummy", "first")));

        assert!(registry.contains("dummy"));
        assert_eq!(registry.len(), 1);
        assert!(registry.unregister("dummy"));
        assert!(!registry.contains("dummy"));
        assert!(registry.is_empty());
    }

    #[test]
    fn test_tool_registry_unregister_missing_tool() {
        let registry = ToolRegistry::new();

        assert!(!registry.unregister("missing"));
        assert!(registry.is_empty());
    }

    #[test]
    fn test_tool_registry_register_overwrites_existing_tool() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool::new("dummy", "first")));
        registry.register(Box::new(DummyTool::new("dummy", "second")));

        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get("dummy").unwrap().description(), "second");
    }
}
