use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
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
    ) -> String;

    /// Single-turn LLM completion (no tool loop). Used for analysis/planning.
    async fn complete(
        &self,
        prompt: &str,
        system: &str,
        ctx: &ToolContext,
    ) -> String;
}

#[derive(Clone)]
pub struct ToolContext {
    pub cwd: PathBuf,
    pub cancel: CancellationToken,
    pub read_files: std::sync::Arc<Mutex<HashSet<String>>>,
    pub agent_runner: Option<std::sync::Arc<dyn AgentRunner>>,
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
