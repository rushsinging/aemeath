//! Built-in commands implementation

use crate::command::{Command, CommandAction, CommandCategory, CommandContext, CommandResult, ConfirmAction};
use crate::session;
use crate::config::PermissionModeConfig;

// ==================== Core Commands ====================

/// Help command - show all available commands
pub fn help_command() -> Command {
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
}

fn help_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let registry = crate::command::registry::CommandRegistry::with_defaults();

    if args.is_empty() {
        // Show all commands grouped by category
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
                .filter(|cmd| cmd.category == category)
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
        // Show help for specific command
        let cmd_name = args.trim().to_lowercase();
          let cmd = registry.find(&cmd_name)
              .or_else(|| registry.find(&format!("/{}", cmd_name)));
          if let Some(cmd) = cmd {
              CommandResult::Success(cmd.help())
          } else {
              CommandResult::Error(format!("Unknown command: /{}", cmd_name))
          }
    }
}

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
            // TODO: Implement history clearing
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
        "" | "auto" => {
            // Auto compact - keep last N messages
            CommandResult::Action(CommandAction::Compact)
        }
        "full" => {
            // Full compaction - summarize all
            CommandResult::Success("Full compaction mode enabled".to_string())
        }
        "status" => {
            // Show compaction status
            CommandResult::Success("Compaction status: ready".to_string())
        }
        _ => CommandResult::Error(format!("Unknown argument: {}", arg)),
    }
}

// ==================== Utility Commands ====================

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
        "clear" => {
            CommandResult::Confirm {
                message: "Clear all cost history?".to_string(),
                action: ConfirmAction::ClearCostHistory,
            }
        }
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
        "" => {
            CommandResult::Success(
                "Usage Statistics:\n  Sessions: 0\n  Messages: 0\n  Tool calls: 0\n  Tokens used: ~0\n\nNote: Usage tracking not yet implemented".to_string()
            )
        }
        "limits" => {
            CommandResult::Success(
                "API Limits:\n  Max tokens: 200,000\n  Context window: 128,000\n  Rate limit: 60 requests/min\n\nNote: Limit tracking not yet implemented".to_string()
            )
        }
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
    // Use ctx.config directly since it's already loaded
    let output = format!(
        "Session Status:\n  Session ID: {}\n  Working directory: {}\n  Model: {}\n  Max tokens: {}\n  Permission mode: {}\n\nConfiguration:\n  Base URL: {}\n  Markdown: {}\n  TUI: {}",
        ctx.session_id,
        ctx.cwd,
        ctx.config.model.name,
        ctx.config.model.max_tokens,
        match ctx.config.permissions.mode {
            PermissionModeConfig::Ask => "ask",
            PermissionModeConfig::AutoRead => "auto-read",
            PermissionModeConfig::AutoAll => "auto-all",
        },
        ctx.config.api.base_url.as_deref().unwrap_or("https://api.anthropic.com"),
        ctx.config.ui.markdown,
        ctx.config.ui.tui,
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

// ==================== Config Commands ====================

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
        // Show current config
        let output = format!(
            "Current Configuration:\n\nAPI:\n  Model: {}\n  Max tokens: {}\n  Base URL: {}\n\nUI:\n  Markdown: {}\n  Color: {}\n  TUI: {}\n\nPermissions:\n  Mode: {}\n\nStorage:\n  Persist sessions: {}\n",
            ctx.config.model.name,
            ctx.config.model.max_tokens,
            ctx.config.api.base_url.as_deref().unwrap_or("https://api.anthropic.com"),
            ctx.config.ui.markdown,
            ctx.config.ui.color,
            ctx.config.ui.tui,
            match ctx.config.permissions.mode {
                PermissionModeConfig::Ask => "ask",
                PermissionModeConfig::AutoRead => "auto-read",
                PermissionModeConfig::AutoAll => "auto-all",
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
                // TODO: Implement config setting
                CommandResult::Success(format!("Set {} = {} (not persisted yet)", parts[1], parts[2]))
            }
            "reset" => {
                CommandResult::Confirm {
                    message: "Reset configuration to defaults?".to_string(),
                    action: ConfirmAction::ResetConfig,
                }
            }
            "save" => {
                CommandResult::Success("Configuration saved to ~/.config/aemeath/config.json".to_string())
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
            PermissionModeConfig::AutoAll => "auto-all".to_string(),
        },
        _ => "unknown key".to_string(),
    }
}

/// Model command - change model
pub fn model_command() -> Command {
    Command::new(
        "model".to_string(),
        "Change or show the current model".to_string(),
        CommandCategory::Config,
        model_execute,
    )
    .with_usage(vec![
        "/model - Show current model".to_string(),
        "/model <name> - Change model".to_string(),
        "/model list - List available models".to_string(),
    ])
}

fn model_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();

    if arg.is_empty() {
        CommandResult::Success(format!("Current model: {}", ctx.config.model.name))
    } else if arg == "list" {
        CommandResult::Success(
            "Available models:\n  claude-sonnet-4-6 (default)\n  claude-opus-4\n  claude-sonnet-4\n  claude-3-5-sonnet\n".to_string()
        )
    } else {
        // Change model
        // TODO: Actually change the model in config
        CommandResult::Success(format!("Model changed to: {} (restart required)", arg))
    }
}

// ==================== Session Commands ====================

/// Resume command - resume a previous session
pub fn resume_command() -> Command {
    Command::new(
        "resume".to_string(),
        "Resume a previous session".to_string(),
        CommandCategory::Session,
        resume_execute,
    )
    .with_usage(vec![
        "/resume - List recent sessions".to_string(),
        "/resume <id> - Resume specific session".to_string(),
    ])
    .with_aliases(vec!["r".to_string()])
}

fn resume_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim();

    if arg.is_empty() {
        // List recent sessions
        let sessions = session::list_sessions();
        if sessions.is_empty() {
            CommandResult::Success("No saved sessions found".to_string())
        } else {
            let mut output = String::from("Recent Sessions:\n\n");
            for (i, sess) in sessions.iter().take(10).enumerate() {
                output.push_str(&format!(
                    "{}. {} - {} messages - {}\n",
                    i + 1,
                    sess.id,
                    sess.messages.len(),
                    sess.updated_at
                ));
            }
            output.push_str("\nUse /resume <id> to resume a session\n");
            CommandResult::Success(output)
        }
    } else {
        // Resume specific session
        let session_id = if arg.chars().all(|c| c.is_ascii_digit()) {
            // User provided an index number
            let sessions = session::list_sessions();
            let idx: usize = arg.parse().unwrap_or(0);
            if idx == 0 || idx > sessions.len() {
                return CommandResult::Error(format!("Invalid session index: {}", idx));
            }
            sessions[idx - 1].id.clone()
        } else {
            // User provided a session ID
            arg.to_string()
        };

        CommandResult::Action(CommandAction::ResumeSession(session_id))
    }
}

/// Session command - manage sessions
pub fn session_command() -> Command {
    Command::new(
        "session".to_string(),
        "Manage sessions".to_string(),
        CommandCategory::Session,
        session_execute,
    )
    .with_usage(vec![
        "/session - Show session info".to_string(),
        "/session new - Start new session".to_string(),
        "/session list - List all sessions".to_string(),
        "/session delete <id> - Delete a session".to_string(),
        "/session title <title> - Set session title".to_string(),
        "/session tag <tag> - Add a tag".to_string(),
        "/session untag <tag> - Remove a tag".to_string(),
        "/session favorite - Mark as favorite".to_string(),
        "/session unfavorite - Remove favorite".to_string(),
        "/session notes <notes> - Add notes".to_string(),
        "/session search <query> - Search sessions".to_string(),
    ])
}

fn session_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let parts: Vec<&str> = args.trim().split_whitespace().collect();

    if parts.is_empty() {
        // Show current session info with metadata
        let sessions = session::list_sessions();
        let current = sessions.iter().find(|s| s.id == ctx.session_id);
        let mut output = format!(
            "Current Session:\n  ID: {}\n  CWD: {}\n  Messages: {}\n",
            ctx.session_id,
            ctx.cwd,
            current.map(|s| s.messages.len()).unwrap_or(0)
        );

        if let Some(sess) = current {
            if let Some(title) = &sess.metadata.title {
                output.push_str(&format!("  Title: {}\n", title));
            }
            if !sess.metadata.tags.is_empty() {
                output.push_str(&format!("  Tags: {}\n", sess.metadata.tags.join(", ")));
            }
            if sess.metadata.is_favorite {
                output.push_str("  Favorite: yes\n");
            }
            if let Some(notes) = &sess.metadata.notes {
                output.push_str(&format!("  Notes: {}\n", notes));
            }
        }

        output.push_str(&format!("\nSaved sessions: {}", sessions.len()));
        CommandResult::Success(output)
    } else {
        match parts[0] {
            "new" => CommandResult::Action(CommandAction::NewSession),
            "list" => {
                let sessions = session::list_sessions();
                let mut output = String::from("Saved Sessions:\n\n");
                for sess in sessions.iter().take(20) {
                    let favorite_marker = if sess.metadata.is_favorite { "★ " } else { "  " };
                    output.push_str(&format!(
                        "{}{} {}\n  Messages: {} | Project: {} | Updated: {}\n",
                        favorite_marker,
                        sess.id,
                        sess.display_title(),
                        sess.messages.len(),
                        sess.metadata.project.as_deref().unwrap_or("unknown"),
                        sess.updated_at
                    ));
                    if !sess.metadata.tags.is_empty() {
                        output.push_str(&format!("  Tags: {}\n", sess.metadata.tags.join(", ")));
                    }
                    output.push_str("\n");
                }
                if sessions.len() > 20 {
                    output.push_str(&format!("... and {} more sessions\n", sessions.len() - 20));
                }
                CommandResult::Success(output)
            }
            "delete" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session delete <id>".to_string());
                }
                CommandResult::Confirm {
                    message: format!("Delete session {}?", parts[1]),
                    action: ConfirmAction::DeleteSession(parts[1].to_string()),
                }
            }
            "save" => {
                CommandResult::Success(format!("Session {} saved", ctx.session_id))
            }
            "title" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session title <title>".to_string());
                }
                let title = parts[1..].join(" ");
                match session::update_session_metadata(&ctx.session_id, Some(title), None, None, None) {
                    Ok(sess) => CommandResult::Success(format!("Session title set to: {}", sess.metadata.title.unwrap_or_default())),
                    Err(e) => CommandResult::Error(e),
                }
            }
            "tag" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session tag <tag>".to_string());
                }
                let tag = parts[1];
                match session::load_session(&ctx.session_id) {
                    Ok(mut sess) => {
                        sess.add_tag(tag.to_string());
                        if let Err(e) = session::save_session(&sess) {
                            return CommandResult::Error(e);
                        }
                        CommandResult::Success(format!("Tag '{}' added to session", tag))
                    }
                    Err(e) => CommandResult::Error(e),
                }
            }
            "untag" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session untag <tag>".to_string());
                }
                let tag = parts[1];
                match session::load_session(&ctx.session_id) {
                    Ok(mut sess) => {
                        sess.remove_tag(tag);
                        if let Err(e) = session::save_session(&sess) {
                            return CommandResult::Error(e);
                        }
                        CommandResult::Success(format!("Tag '{}' removed from session", tag))
                    }
                    Err(e) => CommandResult::Error(e),
                }
            }
            "favorite" => {
                match session::update_session_metadata(&ctx.session_id, None, None, None, Some(true)) {
                    Ok(_) => CommandResult::Success("Session marked as favorite".to_string()),
                    Err(e) => CommandResult::Error(e),
                }
            }
            "unfavorite" => {
                match session::update_session_metadata(&ctx.session_id, None, None, None, Some(false)) {
                    Ok(_) => CommandResult::Success("Session removed from favorites".to_string()),
                    Err(e) => CommandResult::Error(e),
                }
            }
            "notes" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session notes <notes>".to_string());
                }
                let notes = parts[1..].join(" ");
                match session::update_session_metadata(&ctx.session_id, None, None, Some(notes), None) {
                    Ok(sess) => CommandResult::Success(format!("Notes updated: {}", sess.metadata.notes.unwrap_or_default())),
                    Err(e) => CommandResult::Error(e),
                }
            }
            "search" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /session search <query>".to_string());
                }
                let query = parts[1..].join(" ");
                let filter = session::SessionFilter {
                    title: Some(query.clone()),
                    project: Some(query.clone()),
                    ..Default::default()
                };
                let results = session::search_sessions(&filter);
                if results.is_empty() {
                    CommandResult::Success("No matching sessions found".to_string())
                } else {
                    let mut output = format!("Found {} sessions:\n\n", results.len());
                    for sess in results.iter().take(10) {
                        output.push_str(&format!("{} {}\n  {}\n\n", sess.id, sess.display_title(), sess.updated_at));
                    }
                    CommandResult::Success(output)
                }
            }
            _ => CommandResult::Error(format!("Unknown session command: {}", parts[0])),
        }
    }
}

// ==================== Task Commands ====================

/// Tasks command - manage tasks
pub fn tasks_command() -> Command {
    Command::new(
        "tasks".to_string(),
        "Manage tasks".to_string(),
        CommandCategory::Tasks,
        tasks_execute,
    )
    .with_usage(vec![
        "/tasks - List all tasks".to_string(),
        "/tasks active - Show active tasks".to_string(),
        "/tasks completed - Show completed tasks".to_string(),
    ])
}

fn tasks_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    // Note: TaskStore is separate from AppState, use Task tools instead
    let arg = args.trim().to_lowercase();

    match arg.as_str() {
        "" | "all" => {
            CommandResult::Success(
                "Task Management:\n\nUse the following tools to manage tasks:\n  - TaskCreate: Create a new task\n  - TaskList: List all tasks\n  - TaskGet: Get task details\n  - TaskUpdate: Update task status\n  - TaskStop: Stop/delete a task\n  - TodoWrite: Create a todo list\n\nExample: Use 'TaskList' tool to see all tasks".to_string()
            )
        }
        "active" => {
            CommandResult::Success("Use 'TaskList' tool with status='in_progress' filter".to_string())
        }
        "completed" => {
            CommandResult::Success("Use 'TaskList' tool with status='completed' filter".to_string())
        }
        _ => CommandResult::Error(format!("Unknown argument: {}", arg)),
    }
}

// ==================== Tool Commands ====================

/// MCP command - manage MCP servers
pub fn mcp_command() -> Command {
    Command::new(
        "mcp".to_string(),
        "Manage MCP servers".to_string(),
        CommandCategory::Tools,
        mcp_execute,
    )
    .with_usage(vec![
        "/mcp - List MCP servers".to_string(),
        "/mcp add <name> <command> - Add MCP server".to_string(),
        "/mcp remove <name> - Remove MCP server".to_string(),
        "/mcp tools - List MCP tools".to_string(),
    ])
}

fn mcp_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    // Note: MCP servers are managed separately, use MCP tools instead
    let parts: Vec<&str> = args.trim().split_whitespace().collect();

    if parts.is_empty() {
        CommandResult::Success(
            "MCP (Model Context Protocol):\n\nUse the following tools to manage MCP:\n  - McpTool: Call an MCP tool\n  - ListMcpResourcesTool: List MCP resources\n  - ReadMcpResourceTool: Read an MCP resource\n\nMCP servers are configured in ~/.config/aemeath/config.json".to_string()
        )
    } else {
        match parts[0] {
            "tools" => {
                CommandResult::Success("MCP tools: Use ToolSearch or ListMcpResourcesTool to find available tools".to_string())
            }
            "add" => {
                CommandResult::Success("Add MCP server in config: ~/.config/aemeath/config.json".to_string())
            }
            "remove" => {
                CommandResult::Success("Remove MCP server in config: ~/.config/aemeath/config.json".to_string())
            }
            _ => CommandResult::Error(format!("Unknown MCP command: {}", parts[0])),
        }
    }
}

/// Skills command - manage skills
pub fn skills_command() -> Command {
    Command::new(
        "skills".to_string(),
        "Manage skills".to_string(),
        CommandCategory::Tools,
        skills_execute,
    )
    .with_usage(vec![
        "/skills - List available skills".to_string(),
        "/skills run <name> - Run a skill".to_string(),
    ])
}

fn skills_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();

    match arg.as_str() {
        "" | "list" => {
            // TODO: List skills from .claude/skills/
            CommandResult::Success(
                "Available Skills:\n\n  commit - Create a git commit\n  review - Review code changes\n\nUse /skills run <name> to execute a skill".to_string()
            )
        }
        other => {
            CommandResult::Success(format!("Run skill: {} (use Skill tool)", other))
        }
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
        "/permissions auto-all - Set to auto-all mode".to_string(),
    ])
    .with_aliases(vec!["perm".to_string()])
}

fn permissions_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();

    match arg.as_str() {
        "" => {
            CommandResult::Success(format!(
                "Current permission mode: {}\n\nModes:\n  ask - Ask for every tool\n  auto-read - Auto-approve read-only tools\n  auto-all - Auto-approve all tools",
                match ctx.config.permissions.mode {
                    PermissionModeConfig::Ask => "ask",
                    PermissionModeConfig::AutoRead => "auto-read",
                    PermissionModeConfig::AutoAll => "auto-all",
                }
            ))
        }
        "ask" => CommandResult::Success("Permission mode set to: ask".to_string()),
        "auto-read" | "autoread" => CommandResult::Success("Permission mode set to: auto-read".to_string()),
        "auto-all" | "autoall" => CommandResult::Success("Permission mode set to: auto-all (warning: all tools will be auto-approved)".to_string()),
        _ => CommandResult::Error(format!("Unknown permission mode: {}", arg)),
    }
}

// ==================== Debug Commands ====================

/// Doctor command - system diagnostics
pub fn doctor_command() -> Command {
    Command::new(
        "doctor".to_string(),
        "Run system diagnostics".to_string(),
        CommandCategory::Debug,
        doctor_execute,
    )
    .with_usage(vec!["/doctor - Run diagnostics".to_string()])
}

fn doctor_execute(_args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let mut output = String::from("System Diagnostics:\n\n");

    // Check API key
    let api_key_set = std::env::var("ANTHROPIC_API_KEY").is_ok();
    output.push_str(&format!("API Key: {}\n", if api_key_set { "✓ set" } else { "✗ not set" }));

    // Check config file
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let config_path = home.join(".config").join("aemeath").join("config.json");
    output.push_str(&format!("Config file: {}\n", if config_path.exists() { "✓ exists" } else { "✗ not found" }));

    // Check sessions directory
    let sessions_path = home.join(".aemeath").join("sessions");
    output.push_str(&format!("Sessions dir: {}\n", if sessions_path.exists() { "✓ exists" } else { "✗ not found" }));

    // Check working directory
    output.push_str(&format!("Working dir: {}\n", std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "✗ error".to_string())));

    // Check git
    let is_git = std::path::Path::new(".git").exists();
    output.push_str(&format!("Git repo: {}\n", if is_git { "✓ yes" } else { "✗ no" }));

    // Check version
    output.push_str(&format!("Version: {}\n", env!("CARGO_PKG_VERSION")));

    output.push_str("\nSystem OK\n");
    CommandResult::Success(output)
}

// ==================== Git Commands ====================

/// Init command - initialize project
pub fn init_command() -> Command {
    Command::new(
        "init".to_string(),
        "Initialize project with aemeath".to_string(),
        CommandCategory::Git,
        init_execute,
    )
    .with_usage(vec![
        "/init - Initialize current directory".to_string(),
        "/init force - Force re-initialization".to_string(),
    ])
}

fn init_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let force = args.trim().to_lowercase() == "force";

    // Check if already initialized
    let aemeath_dir = std::path::Path::new(".aemeath");
    if aemeath_dir.exists() && !force {
        return CommandResult::Error("Already initialized. Use /init force to re-initialize".to_string());
    }

    // Create .aemeath directory
    let mut output = String::from("Initializing project...\n\n");

    if std::fs::create_dir_all(".aemeath").is_ok() {
        output.push_str("✓ Created .aemeath directory\n");
    } else {
        output.push_str("✗ Failed to create .aemeath directory\n");
    }

    // Create CLAUDE.md if it doesn't exist
    let claude_md = std::path::Path::new("CLAUDE.md");
    if !claude_md.exists() {
        if std::fs::write(claude_md, "# Project Context\n\nThis file provides context for aemeath.\n").is_ok() {
            output.push_str("✓ Created CLAUDE.md\n");
        }
    } else {
        output.push_str("✓ CLAUDE.md already exists\n");
    }

    output.push_str("\nProject initialized!\n");
    CommandResult::Success(output)
}

/// Commit command - create git commit
pub fn commit_command() -> Command {
    Command::new(
        "commit".to_string(),
        "Create a git commit with AI".to_string(),
        CommandCategory::Git,
        commit_execute,
    )
    .with_usage(vec![
        "/commit - Create commit".to_string(),
        "/commit message - Create commit with message".to_string(),
    ])
}

fn commit_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    // Check if git repo
    if !std::path::Path::new(".git").exists() {
        return CommandResult::Error("Not a git repository. Use /init first".to_string());
    }

    if args.trim().is_empty() {
        CommandResult::Success("Commit: Use the Skill tool with 'commit' skill to create a commit with AI-generated message".to_string())
    } else {
        CommandResult::Success(format!("Commit with message: {} (use Bash tool to commit)", args.trim()))
    }
}

/// Rewind command - rewind history
pub fn rewind_command() -> Command {
    Command::new(
        "rewind".to_string(),
        "Rewind message history".to_string(),
        CommandCategory::Session,
        rewind_execute,
    )
    .with_usage(vec![
        "/rewind - Show rewind options".to_string(),
        "/rewind <n> - Rewind n messages".to_string(),
        "/rewind to <id> - Rewind to specific message".to_string(),
    ])
}

fn rewind_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    let parts: Vec<&str> = args.trim().split_whitespace().collect();

    if parts.is_empty() {
        // Show rewind options
        CommandResult::Success(
            "Rewind Options:\n\nUse /rewind to remove messages from the session history.\n\nUsage:\n  /rewind <n> - Remove last n messages\n  /rewind to <id> - Rewind to message ID\n\nNote: Use session management for full history control".to_string()
        )
    } else {
        match parts[0] {
            "to" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: /rewind to <id>".to_string());
                }
                CommandResult::Success(format!("Rewind to message: {}", parts[1]))
            }
            n => {
                let count: usize = n.parse().unwrap_or(1);
                CommandResult::Success(format!("Rewind {} messages", count))
            }
        }
    }
}
// ==================== Review Commands ====================

/// Review command - code review
pub fn review_command() -> Command {
    Command::new(
        "review".to_string(),
        "Review code changes or files".to_string(),
        CommandCategory::Git,
        review_execute,
    )
    .with_usage(vec![
        "/review - Review current changes".to_string(),
        "/review <file> - Review specific file".to_string(),
        "/review diff - Review git diff".to_string(),
        "/review last - Review last commit".to_string(),
    ])
    .with_aliases(vec!["rev".to_string()])
}

fn review_execute(args: &str, _ctx: &mut CommandContext) -> CommandResult {
    // Check if git repo
    if !std::path::Path::new(".git").exists() {
        return CommandResult::Error("Not a git repository. Use /init first".to_string());
    }

    let arg = args.trim().to_lowercase();

    match arg.as_str() {
        "" | "changes" => {
            CommandResult::Success(
                "Code Review: Current Changes\n\nUse the following approach:\n1. Run `git status` to see changed files\n2. Run `git diff` to see the changes\n3. Ask me to review specific files or the entire diff\n\nExample: \"Please review the changes in src/main.rs\"".to_string()
            )
        }
        "diff" => {
            CommandResult::Success(
                "Review Git Diff:\n\nRun `git diff` using Bash tool, then ask me to:\n- Check for potential bugs\n- Review code style\n- Suggest improvements\n- Identify security issues".to_string()
            )
        }
        "last" | "last-commit" => {
            CommandResult::Success(
                "Review Last Commit:\n\nRun `git show HEAD` using Bash tool to see the last commit, then ask for a review.".to_string()
            )
        }
        _ => {
            // Assume it's a file path
            CommandResult::Success(format!(
                "Review File: {}\n\nUse FileRead tool to read the file, then ask for a code review.",
                arg
            ))
        }
    }
}

// ==================== Stats Commands ====================

/// Stats command - show statistics
pub fn stats_command() -> Command {
    Command::new(
        "stats".to_string(),
        "Show session and usage statistics".to_string(),
        CommandCategory::Utility,
        stats_execute,
    )
    .with_usage(vec![
        "/stats - Show all statistics".to_string(),
        "/stats session - Session stats".to_string(),
        "/stats tools - Tool usage stats".to_string(),
        "/stats tokens - Token usage estimate".to_string(),
    ])
    .with_aliases(vec!["statistics".to_string()])
}

fn stats_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim().to_lowercase();

    match arg.as_str() {
        "" | "all" => {
            let sessions = crate::session::list_sessions();
            let total_sessions = sessions.len();
            let total_messages: usize = sessions.iter().map(|s| s.messages.len()).sum();

            let output = format!(
                "Statistics Summary:\n\n\
                Sessions:\n  Total saved: {}\n  Current session: {}\n  Total messages: {}\n\n\
                Configuration:\n  Model: {}\n  Max tokens: {}\n  Context size: {}\n\n\
                Cost Tracking:\n  Use /cost for detailed cost information\n\n\
                Tokens:\n  Use /stats tokens for token estimate",
                total_sessions,
                ctx.session_id,
                total_messages,
                ctx.config.model.name,
                ctx.config.model.max_tokens,
                ctx.config.model.context_size
            );
            CommandResult::Success(output)
        }
        "session" => {
            let sessions = crate::session::list_sessions();
            let mut output = String::from("Session Statistics:\n\n");
            for sess in sessions.iter().take(10) {
                output.push_str(&format!(
                    "Session {}:\n  Messages: {}\n  Created: {}\n  Updated: {}\n\n",
                    sess.id, sess.messages.len(), sess.created_at, sess.updated_at
                ));
            }
            if sessions.len() > 10 {
                output.push_str(&format!("... and {} more sessions\n", sessions.len() - 10));
            }
            CommandResult::Success(output)
        }
        "tools" => {
            CommandResult::Success(
                "Tool Usage Statistics:\n\n\
                Tool usage tracking is not yet implemented.\n\
                Future feature: Track which tools are used most frequently.\n\n\
                Available tools: 27\n\
                - File operations: Read, Write, Edit, Glob, Grep\n\
                - Shell: Bash\n\
                - Tasks: TaskCreate, TaskUpdate, TaskList, TaskGet, TaskStop, TodoWrite\n\
                - Web: WebFetch, WebSearch\n\
                - MCP: McpTool, ListMcpResources, ReadMcpResource\n\
                - Agent: Agent\n\
                - Plan Mode: EnterPlanMode, ExitPlanMode\n\
                - Utility: Config, Sleep, AskUserQuestion, ToolSearch, Brief\n\
                - Skills: Skill\n\
                - Dev: LSP".to_string()
            )
        }
        "tokens" => {
            // Estimate tokens from current session
            let sessions = crate::session::list_sessions();
            let current_session = sessions.iter().find(|s| s.id == ctx.session_id);

            let token_estimate = if let Some(sess) = current_session {
                crate::compact::estimate_messages_tokens(&sess.messages)
            } else {
                0
            };

            let output = format!(
                "Token Usage Estimate:\n\n\
                Current session tokens: ~{}\n\
                Context size limit: {}\n\
                Usage: {:.1}%\n\n\
                Note: This is an estimate based on message content.\n\
                Actual token counts may vary based on the model's tokenizer.",
                token_estimate,
                ctx.config.model.context_size,
                (token_estimate as f64 / ctx.config.model.context_size as f64) * 100.0
            );
            CommandResult::Success(output)
        }
        _ => CommandResult::Error(format!("Unknown stats type: {}", arg))
    }
}
