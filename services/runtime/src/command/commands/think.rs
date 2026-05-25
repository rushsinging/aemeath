//! Think command — toggle reasoning/thinking mode.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::command::{
    Command, CommandAction, CommandCategory, CommandContext, CommandDescriptor, CommandResult,
};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "think".to_string(),
            "Toggle reasoning/thinking mode".to_string(),
            CommandCategory::Config,
            think_execute,
        )
        .with_usage(vec![
            "/think - Toggle thinking mode on/off".to_string(),
            "/think on - Enable thinking mode".to_string(),
            "/think off - Disable thinking mode".to_string(),
        ])
    })
}

fn think_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let new_state = match args.trim() {
        "on" | "true" | "1" => Some(true),
        "off" | "false" | "0" => Some(false),
        "" => None, // toggle — handled by the caller
        other => {
            return CommandResult::Error(format!(
                "Unknown argument: {}. Use on/off or omit to toggle.",
                other
            ))
        }
    };
    CommandResult::Action(CommandAction::SetThinking(new_state))
}
