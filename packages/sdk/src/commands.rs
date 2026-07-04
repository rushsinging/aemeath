//! Slash command metadata exposed to TUI completion + command execution DTOs.

// ─── Builtin command metadata (TUI autocomplete) ───

pub fn builtin_commands() -> Vec<(String, String, Vec<String>)> {
    [
        ("help", "Show available commands", vec![]),
        ("clear", "Clear the current conversation", vec![]),
        ("compact", "Compact the current conversation", vec![]),
        ("usage", "Show current token usage", vec![]),
        ("model", "Switch model", vec![]),
        ("context", "Show context window usage", vec![]),
        ("cost", "Show API cost statistics", vec![]),
        ("status", "Show current session status", vec![]),
        ("config", "Manage configuration settings", vec![]),
        ("stats", "Show statistics", vec![]),
        ("init", "Initialize project", vec![]),
        ("session", "Manage sessions", vec![]),
        ("resume", "Resume a previous session", vec![]),
        ("memory", "Manage memory", vec![]),
        ("version", "Show version information", vec![]),
        ("doctor", "Run system diagnostics", vec![]),
        ("rewind", "Rewind conversation", vec![]),
        ("save", "Save current session", vec![]),
        ("reflect", "Run reflection", vec![]),
        ("paste", "Paste image from clipboard", vec![]),
        ("images", "List pending images", vec![]),
        ("clear-images", "Clear pending images", vec![]),
        ("exit", "Exit the application", vec![]),
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
