//! Slash command metadata exposed to TUI completion + command execution DTOs.

use serde::{Deserialize, Serialize};

// ─── Builtin command metadata (TUI autocomplete) ───

pub fn builtin_commands() -> Vec<(String, String, Vec<String>)> {
    [
        ("help", "Show available commands", vec!["h"]),
        ("clear", "Clear the current conversation", vec![]),
        ("compact", "Compact the current conversation", vec![]),
        ("usage", "Show current token usage", vec![]),
        ("model", "Switch model", vec![]),
        ("models", "List configured models", vec![]),
        ("resume", "Resume a previous session", vec![]),
        ("sessions", "List previous sessions", vec![]),
        ("save", "Save current session", vec![]),
        ("context", "Show context window usage", vec![]),
        ("reflect", "Run reflection", vec![]),
        ("memory", "Manage memory", vec!["mem"]),
        ("paste", "Paste image from clipboard", vec![]),
        ("images", "List pending images", vec![]),
        ("clear-images", "Clear pending images", vec![]),
        ("exit", "Exit the application", vec!["quit"]),
    ]
    .into_iter()
    .map(|(name, description, aliases)| {
        (
            name.to_string(),
            description.to_string(),
            aliases.into_iter().map(str::to_string).collect(),
        )
    })
    .collect()
}

// ─── Command execution types (SDK DTO) ───

/// Context passed from CLI to Runtime for command execution.
#[derive(Debug, Clone)]
pub struct CommandContext {
    /// Current working directory
    pub cwd: String,
    /// Current session ID
    pub session_id: String,
    /// Available model summaries (for model-switch autocomplete etc.)
    pub models: Vec<super::ModelSummary>,
    /// Current model display name
    pub current_model: String,
}

/// Result of executing a command in Runtime.
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// Command produced output text
    Success(String),
    /// Command requires caller to handle an action
    Action(CommandAction),
    /// Command failed
    Error(String),
    /// Command needs confirmation
    Confirm {
        message: String,
        action: ConfirmAction,
    },
}

/// Side-effect actions that Runtime commands want the CLI to perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandAction {
    Exit,
    Clear,
    Compact,
    /// Resume a previous session
    ResumeSession(String),
    NewSession,
    ChangeMode(String),
    /// Switch to a different model (Runtime builds the new client)
    SwitchModel {
        provider_name: String,
        model_id: String,
        model_name: String,
        base_url: String,
        api_key: String,
        api_type: String,
        max_tokens: u32,
        context_window: usize,
        reasoning: Option<bool>,
    },
    /// Inject a user message into the conversation
    InjectMessage(String),
    /// Run a skill (content injected as user message)
    RunSkill(String),
    /// Toggle / set reasoning mode (None = toggle)
    SetThinking(Option<bool>),
}

/// Confirmation-required actions from commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfirmAction {
    DeleteSession(String),
    ClearAllHistory,
    ResetConfig,
    ClearCostHistory,
}

// ─── Context estimation (SDK DTO) ───

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

// ─── Model switch params / result ───

/// Parameters for switching the active model.
#[derive(Debug, Clone)]
pub struct ModelSwitchParams {
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub base_url: String,
    pub api_key: String,
    pub api_type: String,
    pub max_tokens: u32,
    pub context_window: usize,
    pub reasoning: Option<bool>,
}

/// Result of switching the active model.
#[derive(Debug, Clone)]
pub struct ModelSwitchResult {
    pub display_name: String,
    pub context_window: usize,
    pub reasoning_active: Option<bool>,
}
