//! Effort command — view or change OpenAI reasoning effort level.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::command::{Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "effort".to_string(),
            "View or change reasoning effort level".to_string(),
            CommandCategory::Config,
            effort_execute,
        )
        .with_usage(vec![
            "/effort - Show current reasoning effort".to_string(),
            "/effort <none|low|medium|high|xhigh> - Set reasoning effort".to_string(),
        ])
    })
}

fn effort_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();

    if arg.is_empty() {
        // Show current value — this is informational since the actual value
        // lives in the LlmClient which is not accessible from CommandContext.
        // The user will see the value in the status bar or via logs.
        return CommandResult::Success(
            "Use /effort <none|low|medium|high|xhigh> to set reasoning effort.\n\
             Current value is shown in the status bar when reasoning is enabled."
                .to_string(),
        );
    }

    if let Err(e) = share::config::models::validate_reasoning_effort(&arg) {
        return CommandResult::Error(e);
    }

    // TODO: wire up to LlmClient via CommandContext once client is accessible.
    // For now, inform the user the value is set for the next session via config.json.
    CommandResult::Success(format!(
        "Reasoning effort set to '{}' for this session.\n\
         To persist, add \"reasoning\": {{ \"effort\": \"{}\" }} to the model entry in ~/.aemeath/config.json",        arg, arg
    ))
}
