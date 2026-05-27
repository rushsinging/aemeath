//! Slash command metadata exposed to TUI completion.

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
