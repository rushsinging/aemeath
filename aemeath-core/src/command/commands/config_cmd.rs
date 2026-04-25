use crate::command::{Command, CommandAction, CommandCategory, CommandContext, CommandResult, ConfirmAction};
use crate::config::PermissionModeConfig;

/// Config command - manage configuration
pub fn config_command() -> Command {
    Command::new(
        "config".to_string(),
        "Manage configuration settings".to_string(),
        CommandCategory::Config,
        config_execute,
    )
    .with_usage(vec![
        "/config - Show current config".to_string(),
        "/config get <key> - Get a config value".to_string(),
        "/config set <key> <value> - Set a config value".to_string(),
        "/config reset - Reset to defaults".to_string(),
        "/config save - Save current config".to_string(),
    ])
    .with_aliases(vec!["cfg".to_string()])
}

fn config_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let parts: Vec<&str> = args.trim().split_whitespace().collect();
    if parts.is_empty() {
        let output = format!(
            "Current Configuration:\n\n\
             API:\n  Model: {}\n  Max tokens: {}\n  Base URL: {}\n\n\
             UI:\n  Markdown: {}\n  Color: {}\n  TUI: {}\n\n\
             Permissions:\n  Mode: {}\n\n\
             Storage:\n  Persist sessions: {}\n",
            ctx.config.model.name,
            ctx.config.model.max_tokens,
            ctx.config.api.base_url.as_deref().unwrap_or("https://api.anthropic.com"),
            ctx.config.ui.markdown,
            ctx.config.ui.color,
            ctx.config.ui.tui,
            match ctx.config.permissions.mode {
                PermissionModeConfig::Ask => "ask",
                PermissionModeConfig::AutoRead => "auto-read",
                PermissionModeConfig::AllowAll => "allow-all",
            },
            ctx.config.storage.persist_sessions,
        );
        CommandResult::Success(output)
    } else {
        match parts[0] {
            "get" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /config get <key>".to_string());
                }
                let key = parts[1];
                let value = get_config_value(&ctx.config, key);
                CommandResult::Success(format!("{} = {}", key, value))
            }
            "set" => {
                if parts.len() < 3 {
                    return CommandResult::Error("Usage: /config set <key> <value>".to_string());
                }
                CommandResult::Error(format!(
                    "/config set is not yet implemented. Edit ~/.aemeath/config.json directly."
                ))
            }
            "reset" => {
                CommandResult::Confirm {
                    message: "Reset configuration to defaults?".to_string(),
                    action: ConfirmAction::ResetConfig,
                }
            }
            "save" => {
                CommandResult::Error(
                    "/config save is not yet implemented. Edit ~/.aemeath/config.json directly.".to_string()
                )
            }
            _ => CommandResult::Error(format!("Unknown config command: {}", parts[0])),
        }
    }
}

fn get_config_value(config: &crate::config::Config, key: &str) -> String {
    match key {
        "model" => config.model.name.clone(),
        "max_tokens" => config.model.max_tokens.to_string(),
        "base_url" => config.api.base_url.clone().unwrap_or_else(|| "default".to_string()),
        "temperature" => config.model.temperature.map(|t| t.to_string()).unwrap_or_else(|| "default".to_string()),
        "context_size" => config.model.context_size.to_string(),
        "permission_mode" => match config.permissions.mode {
            PermissionModeConfig::Ask => "ask".to_string(),
            PermissionModeConfig::AutoRead => "auto-read".to_string(),
            PermissionModeConfig::AllowAll => "allow-all".to_string(),
        },
        _ => "unknown key".to_string(),
    }
}

/// Permissions command - manage permissions
pub fn permissions_command() -> Command {
    Command::new(
        "permissions".to_string(),
        "Manage permission settings".to_string(),
        CommandCategory::Config,
        permissions_execute,
    )
    .with_usage(vec![
        "/permissions - Show current mode".to_string(),
        "/permissions ask - Set to ask mode".to_string(),
        "/permissions auto-read - Set to auto-read mode".to_string(),
        "/permissions allow-all - Set to allow-all mode".to_string(),
    ])
    .with_aliases(vec!["perm".to_string()])
}

fn permissions_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();
    match arg.as_str() {
        "" => {
            CommandResult::Success(format!(
                "Current permission mode: {}\n\nModes:\n  ask - Ask for every tool\n  auto-read - Auto-approve read-only tools\n  allow-all - Auto-approve all tools",
                match ctx.config.permissions.mode {
                    PermissionModeConfig::Ask => "ask",
                    PermissionModeConfig::AutoRead => "auto-read",
                    PermissionModeConfig::AllowAll => "allow-all",
                }
            ))
        }
        "ask" => CommandResult::Action(CommandAction::ChangeMode("ask".to_string())),
        "auto-read" | "autoread" => CommandResult::Action(CommandAction::ChangeMode("auto-read".to_string())),
        "allow-all" | "auto-all" | "autoall" => CommandResult::Action(CommandAction::ChangeMode("allow-all".to_string())),
        _ => CommandResult::Error(format!("Unknown permission mode: {}", arg)),
    }
}
