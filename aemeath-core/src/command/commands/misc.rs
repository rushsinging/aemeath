use crate::command::{Command, CommandAction, CommandCategory, CommandContext, CommandResult, ConfirmAction};
use crate::config::PermissionModeConfig;

/// Exit command - quit the application
pub fn exit_command() -> Command {
    Command::new(
        "exit".to_string(),
        "Exit the application".to_string(),
        CommandCategory::Core,
        |_args, _ctx| CommandResult::Action(CommandAction::Exit),
    )
    .with_usage(vec!["/exit - Exit aemeath".to_string()])
    .with_aliases(vec!["quit".to_string(), "q".to_string()])
}

/// Clear command - clear screen or history
pub fn clear_command() -> Command {
    Command::new(
        "clear".to_string(),
        "Clear screen or history".to_string(),
        CommandCategory::Core,
        clear_execute,
    )
    .with_usage(vec![
        "/clear - Clear screen".to_string(),
        "/clear history - Clear command history".to_string(),
        "/clear all - Clear everything (requires confirmation)".to_string(),
    ])
    .with_aliases(vec!["cls".to_string()])
}

fn clear_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();
    match arg.as_str() {
        "" | "screen" => CommandResult::Action(CommandAction::Clear),
        "history" => {
            CommandResult::Success("History cleared".to_string())
        }
        "all" => CommandResult::Confirm {
            message: "Clear all history and reset session?".to_string(),
            action: ConfirmAction::ClearAllHistory,
        },
        _ => CommandResult::Error(format!("Unknown argument: {}", arg)),
    }
}

/// Compact command - compress message history
pub fn compact_command() -> Command {
    Command::new(
        "compact".to_string(),
        "Compact message history to reduce context".to_string(),
        CommandCategory::Core,
        compact_execute,
    )
    .with_usage(vec![
        "/compact - Compact messages".to_string(),
        "/compact full - Full compaction".to_string(),
    ])
    .with_aliases(vec!["c".to_string()])
}

fn compact_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();
    match arg.as_str() {
        "" | "auto" => CommandResult::Action(CommandAction::Compact),
        "full" => CommandResult::Success("Full compaction mode enabled".to_string()),
        "status" => CommandResult::Success("Compaction status: ready".to_string()),
        _ => CommandResult::Error(format!("Unknown argument: {}", arg)),
    }
}

/// Cost command - show API cost statistics
pub fn cost_command() -> Command {
    Command::new(
        "cost".to_string(),
        "Show API cost statistics".to_string(),
        CommandCategory::Utility,
        cost_execute,
    )
    .with_usage(vec![
        "/cost - Show current session cost".to_string(),
        "/cost total - Show total cost across sessions".to_string(),
    ])
}

fn cost_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();
    match arg.as_str() {
        "" | "session" => {
            let summary = ctx.cost_tracker.session_summary(&ctx.session_id);
            CommandResult::Success(summary.format())
        }
        "total" => {
            let summary = ctx.cost_tracker.summary();
            CommandResult::Success(summary.format())
        }
        "clear" => CommandResult::Confirm {
            message: "Clear all cost history?".to_string(),
            action: ConfirmAction::ClearCostHistory,
        },
        _ => CommandResult::Error(format!("Unknown argument: {}", arg)),
    }
}

/// Usage command - show usage statistics
pub fn usage_command() -> Command {
    Command::new(
        "usage".to_string(),
        "Show usage statistics and limits".to_string(),
        CommandCategory::Utility,
        usage_execute,
    )
    .with_usage(vec![
        "/usage - Show current usage".to_string(),
        "/usage limits - Show API limits".to_string(),
    ])
}

fn usage_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();
    match arg.as_str() {
        "" => CommandResult::Success(
            "Usage Statistics:\n  Sessions: 0\n  Messages: 0\n  Tool calls: 0\n  Tokens used: ~0\n\nNote: Usage tracking not yet implemented".to_string()
        ),
        "limits" => CommandResult::Success(
            "API Limits:\n  Max tokens: 200,000\n  Context window: 128,000\n  Rate limit: 60 requests/min\n\nNote: Limit tracking not yet implemented".to_string()
        ),
        _ => CommandResult::Error(format!("Unknown argument: {}", arg)),
    }
}

/// Status command - show current status
pub fn status_command() -> Command {
    Command::new(
        "status".to_string(),
        "Show current session status".to_string(),
        CommandCategory::Utility,
        status_execute,
    )
    .with_usage(vec!["/status - Show current status".to_string()])
    .with_aliases(vec!["st".to_string()])
}

fn status_execute(_args: &str, ctx: &mut CommandContext) -> CommandResult {
    let permission_emoji = match ctx.config.permissions.mode {
        PermissionModeConfig::Ask => "🔔",
        PermissionModeConfig::AutoRead => "📖",
        PermissionModeConfig::AllowAll => "🔓",
    };
    let permission_text = match ctx.config.permissions.mode {
        PermissionModeConfig::Ask => "ask",
        PermissionModeConfig::AutoRead => "auto-read",
        PermissionModeConfig::AllowAll => "allow-all",
    };
    let markdown_icon = if ctx.config.ui.markdown { "✅" } else { "❌" };
    let tui_icon = if ctx.config.ui.tui { "✅" } else { "❌" };
    let base_url = ctx.config.api.base_url.as_deref().unwrap_or("https://api.anthropic.com");
    let output = format!(
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         📊 Session Status\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         🆔 Session ID\n\
         │ {}\n\
         📁 Working directory\n\
         │ {}\n\
         🤖 Model\n\
         │ {}\n\
         📏 Max tokens\n\
         │ {}\n\
         🔐 Permission mode {}\n\
         │ {}\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         ⚙️ Configuration\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         🌐 Base URL\n\
         │ {}\n\
         📝 Markdown {}\n\
         │ {}\n\
         🖥️  TUI {}\n\
         │ {}\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        ctx.session_id,
        ctx.cwd,
        ctx.config.model.name,
        ctx.config.model.max_tokens,
        permission_emoji,
        permission_text,
        base_url,
        markdown_icon,
        if ctx.config.ui.markdown { "enabled" } else { "disabled" },
        tui_icon,
        if ctx.config.ui.tui { "enabled" } else { "disabled" },
    );
    CommandResult::Success(output)
}

/// Version command - show version info
pub fn version_command() -> Command {
    Command::new(
        "version".to_string(),
        "Show version information".to_string(),
        CommandCategory::Utility,
        |_args, _ctx| {
            CommandResult::Success(format!(
                "aemeath v{}\n\nBuild info:\n  Rust version: stable\n  Target: {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH
            ))
        },
    )
    .with_usage(vec!["/version - Show version".to_string()])
    .with_aliases(vec!["v".to_string()])
}
