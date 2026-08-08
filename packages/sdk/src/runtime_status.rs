use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextDecisionSourceView {
    ActualProviderUsage,
    HeuristicFallback,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextBudgetView {
    pub context_size: u64,
    pub effective_window: u64,
    pub decision_token_count: u64,
    pub threshold: u64,
    pub usage_permille: u32,
    pub compaction_needed: bool,
    pub source: ContextDecisionSourceView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeStatusView {
    pub session_id: String,
    pub revision: u64,
    pub heartbeat_sequence: u64,
    pub context_budget: ContextBudgetView,
}
