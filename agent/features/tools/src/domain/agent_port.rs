use crate::domain::{
    CancellationSignal, ExecutionScope, Guidance, PlanModeState, ProgressSink, ReadSet,
};
use async_trait::async_trait;
use std::sync::Arc;
#[derive(Clone)]
pub struct AgentRunRequest<'a> {
    pub prompt: &'a str,
    pub system: &'a str,
    pub identity: &'a ExecutionScope,
    pub cancellation: Arc<dyn CancellationSignal>,
    pub progress: Option<Arc<dyn ProgressSink>>,
    pub memory: Arc<dyn memory::MemoryPort>,
    pub catalog: Option<Arc<dyn crate::domain::CatalogQuery>>,
    pub read_set: Arc<dyn ReadSet>,
    pub plan_mode: Arc<dyn PlanModeState>,
    pub guidance: Arc<dyn Guidance>,
    pub timeout: std::time::Duration,
    pub role: &'a str,
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
            Self::Cancelled => "Cancelled by user".into(),
        }
    }
}
#[async_trait]
pub trait AgentDispatch: Send + Sync {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> AgentRunTerminal;
    async fn complete(
        &self,
        prompt: &str,
        system: &str,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> String;
}
pub use AgentDispatch as AgentRunner;
