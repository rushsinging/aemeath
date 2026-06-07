use async_trait::async_trait;
use share::tool::AgentProgressEvent;

use super::context::ToolExecutionContext;

#[derive(Clone)]
pub struct AgentRunRequest<'a> {
    pub prompt: &'a str,
    pub system: &'a str,
    pub ctx: &'a ToolExecutionContext,
    pub max_turns: Option<u32>,
    pub model_spec: Option<&'a str>,
    /// Optional channel to stream per-turn progress to TUI
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
}

/// Callback for running a sub-agent loop. Implemented by the runtime layer.
#[async_trait]
pub trait AgentRunner: Send + Sync {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> String;

    /// Single-turn LLM completion (no tool loop). Used for analysis/planning.
    async fn complete(&self, prompt: &str, system: &str, ctx: &ToolExecutionContext) -> String;
}
