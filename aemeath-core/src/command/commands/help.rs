//! Help command — shows available commands and usage.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::command::{Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "help".to_string(),
            "Show available commands and usage".to_string(),
            CommandCategory::Core,
            help_execute,
        )
        .with_usage(vec![
            "/help - Show all commands".to_string(),
            "/help <command> - Show help for specific command".to_string(),
        ])
        .with_aliases(vec!["h".to_string(), "?".to_string()])
    })
}

fn help_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let registry = crate::command::CommandRegistry::global();

    if args.is_empty() {
        let mut output = String::from("Available Commands:\n\n");
        let categories = [
            (CommandCategory::Core, "Core Commands"),
            (CommandCategory::Session, "Session Commands"),
            (CommandCategory::Config, "Config Commands"),
            (CommandCategory::Tasks, "Task Commands"),
            (CommandCategory::Tools, "Tool Commands"),
            (CommandCategory::Git, "Git Commands"),
            (CommandCategory::Utility, "Utility Commands"),
            (CommandCategory::Debug, "Debug Commands"),
        ];
        let all_commands = registry.list();
        for (category, label) in categories {
            let commands: Vec<_> = all_commands
                .iter()
                .filter(|c| c.category == category)
                .collect();
            if !commands.is_empty() {
                output.push_str(&format!("{}\n", label));
                for cmd in commands {
                    output.push_str(&format!("  /{} - {}\n", cmd.name, cmd.description));
                }
                output.push_str("\n");
            }
        }
        output.push_str("Use /help <command> for detailed usage.\n");
        CommandResult::Success(output)
    } else {
        let cmd_name = args.trim().to_lowercase();
        if let Some(cmd) = registry
            .find(&cmd_name)
            .or_else(|| registry.find(&format!("/{}", cmd_name)))
        {
            CommandResult::Success(cmd.help())
        } else {
            CommandResult::Error(format!("Unknown command: /{}", cmd_name))
        }
    }
}
