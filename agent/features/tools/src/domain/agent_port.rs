use crate::domain::AgentProgressEvent;
use async_trait::async_trait;

use super::context::ToolExecutionContext;

#[derive(Clone)]
pub struct AgentRunRequest<'a> {
    pub prompt: &'a str,
    pub system: &'a str,
    pub ctx: &'a ToolExecutionContext,
    pub timeout: std::time::Duration,
    pub model_spec: Option<&'a str>,
    /// Optional channel to stream per-turn progress to TUI
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRunTerminal {
    Completed { result: String },
    Failed { error: String },
    Cancelled,
}

impl AgentRunTerminal {
    pub fn output(&self) -> String {
        match self {
            Self::Completed { result } => result.clone(),
            Self::Failed { error } => format!("Sub-agent error: {error}"),
            Self::Cancelled => "Cancelled by user".to_string(),
        }
    }
}

/// Callback for running a sub-agent loop. Implemented by the runtime layer.
#[async_trait]
pub trait AgentRunner: Send + Sync {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> AgentRunTerminal;

    /// Single-turn LLM completion (no tool loop). Used for analysis/planning.
    async fn complete(&self, prompt: &str, system: &str, ctx: &ToolExecutionContext) -> String;
}
