use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

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

#[derive(Clone)]
pub struct ToolContext {
    pub cwd: PathBuf,
    /// Base directory used to resolve relative file/tool paths.
    pub path_base: Arc<Mutex<PathBuf>>,
    pub cancel: CancellationToken,
    pub read_files: std::sync::Arc<Mutex<HashSet<String>>>,
    pub agent_runner: Option<std::sync::Arc<dyn AgentRunner>>,
    /// Session-local reminders shared by MemoryTool and UI/REPL.
    pub session_reminders: Option<Arc<Mutex<crate::memory::SessionReminders>>>,
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
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn unregister(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn schemas(&self) -> Vec<Value> {
        self.tools
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

    pub fn names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
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
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool::new("dummy", "first")));

        assert!(registry.contains("dummy"));
        assert_eq!(registry.len(), 1);
        assert!(registry.unregister("dummy"));
        assert!(!registry.contains("dummy"));
        assert!(registry.is_empty());
    }

    #[test]
    fn test_tool_registry_unregister_missing_tool() {
        let mut registry = ToolRegistry::new();

        assert!(!registry.unregister("missing"));
        assert!(registry.is_empty());
    }

    #[test]
    fn test_tool_registry_register_overwrites_existing_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool::new("dummy", "first")));
        registry.register(Box::new(DummyTool::new("dummy", "second")));

        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get("dummy").unwrap().description(), "second");
    }
}
