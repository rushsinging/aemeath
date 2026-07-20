//! Slash Command Published Language exposed to delivery clients.
//!
//! The owner is the Tools BC. SDK re-exports the exact types and ports instead
//! of defining a second descriptor, parser, or route model.

pub use tools::{
    ApplicationControlCommand, ApplicationControlTarget, CommandArgumentSchema, CommandCatalogPort,
    CommandCompletion, CommandDescriptor, CommandMechanism, CommandName, CommandParseError,
    CommandRoute, CommandRouterPort, CommandTarget, ParsedArguments, PromptCommand, SlashInput,
    SnapshotQueryCommand, SnapshotQueryTarget,
};

// ─── Command execution result views ───

/// Result of estimating context window usage.
#[derive(Debug, Clone, Copy)]
pub struct ContextEstimate {
    /// Estimated token count for all messages
    pub estimated_tokens: usize,
    /// Estimated tokens for system prompt only
    pub system_tokens: usize,
    /// Available context window size
    pub context_size: usize,
    /// Usage percentage (0-100)
    pub usage_percentage: f64,
}

// ─── Model switch result ───

/// Result of switching the active model.
#[derive(Debug, Clone)]
pub struct ModelSwitchResult {
    pub display_name: String,
    pub context_window: usize,
    pub reasoning_active: Option<bool>,
}
