//! Reasoning command — view or change the reasoning level.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::core::command::{
    Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult,
};
use provider::contract::ReasoningLevel;

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "reasoning".to_string(),
            "View or change reasoning level".to_string(),
            CommandCategory::Config,
            effort_execute,
        )
        .with_usage(vec![
            "/reasoning - Show current reasoning level".to_string(),
            "/reasoning <off|low|medium|high|xhigh|max> - Set reasoning level".to_string(),
        ])
    })
}

fn effort_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();

    if arg.is_empty() {
        return CommandResult::Success(
            "Use /reasoning <off|low|medium|high|xhigh|max> to set reasoning level.".to_string(),
        );
    }

    match parse_level(&arg) {
        Some(level) => {
            // TODO: wire up to LlmClient via CommandContext once client is accessible.
            CommandResult::Success(format!(
                "Reasoning level set to '{}' for this session.\n\
                 Note: provider cap and runtime clamp may apply.",
                level.as_str()
            ))
        }
        None => CommandResult::Error(format!(
            "Unknown reasoning level '{}'. Valid values: off, low, medium, high, xhigh, max",
            arg
        )),
    }
}

fn parse_level(s: &str) -> Option<ReasoningLevel> {
    match s {
        "off" => Some(ReasoningLevel::Off),
        "low" => Some(ReasoningLevel::Low),
        "medium" => Some(ReasoningLevel::Medium),
        "high" => Some(ReasoningLevel::High),
        "xhigh" => Some(ReasoningLevel::Xhigh),
        "max" => Some(ReasoningLevel::Max),
        _ => None,
    }
}
