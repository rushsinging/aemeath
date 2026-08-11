#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiContextBudget {
    pub(crate) context_size: u64,
    pub(crate) effective_window: u64,
    pub(crate) decision_token_count: u64,
    pub(crate) threshold: u64,
    pub(crate) usage_permille: u32,
    pub(crate) compaction_needed: bool,
    pub(crate) source: TuiContextDecisionSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiContextDecisionSource {
    ActualProviderUsage,
    HeuristicFallback,
    Manual,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiRuntimeStatus {
    pub(crate) session_id: String,
    pub(crate) revision: u64,
    pub(crate) heartbeat_sequence: u64,
    pub(crate) context_budget: TuiContextBudget,
}
