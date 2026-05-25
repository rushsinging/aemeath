//! MCP commands.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::command::{Command, CommandCategory, CommandContext, CommandDescriptor, CommandResult};

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "mcp".to_string(),
            "Manage MCP servers".to_string(),
            CommandCategory::Tools,
            mcp_execute,
        )
        .with_usage(vec![
            "/mcp - List MCP servers".to_string(),
            "/mcp tools [server] - List MCP tools".to_string(),
            "/mcp restart <server> - Restart MCP server".to_string(),
            "/mcp add <name> <command-or-url> [args...] - Add MCP server".to_string(),
            "/mcp remove <name> - Remove MCP server".to_string(),
        ])
    })
}

fn mcp_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    execute_mcp_command(args, ctx)
}

pub fn execute_mcp_command(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let parts: Vec<&str> = args.split_whitespace().collect();
    match parts.as_slice() {
        [] => CommandResult::Success(format_mcp_servers_from_context(ctx)),
        ["tools"] => CommandResult::Success(format_mcp_tools_from_context(ctx, None)),
        ["tools", server] => CommandResult::Success(format_mcp_tools_from_context(ctx, Some(server))),
        ["tools", ..] => CommandResult::Error("Usage: /mcp tools [server]".to_string()),
        ["restart"] => CommandResult::Error("Usage: /mcp restart <server>".to_string()),
        ["restart", server] => CommandResult::Success(format!(
            "MCP restart requested for server '{}'.\n  Runtime MCP manager status is not attached to command context yet; restart will require the runtime bridge.",
            server
        )),
        ["restart", ..] => CommandResult::Error("Usage: /mcp restart <server>".to_string()),
        ["add"] | ["add", _] => {
            CommandResult::Error("Usage: /mcp add <name> <command-or-url> [args...]".to_string())
        }
        ["add", name, target, rest @ ..] => {
            CommandResult::Success(format_mcp_add_preview(name, target, rest))
        }
        ["remove"] => CommandResult::Error("Usage: /mcp remove <name>".to_string()),
        ["remove", name] => CommandResult::Success(format!(
            "MCP remove requested for server '{}'.\n  Runtime MCP manager status is not attached to command context yet; removal will require the runtime bridge.",
            name
        )),
        ["remove", ..] => CommandResult::Error("Usage: /mcp remove <name>".to_string()),
        [unknown, ..] => CommandResult::Error(format!("Unknown MCP command: {}", unknown)),
    }
}

fn format_mcp_servers_from_context(ctx: &CommandContext) -> String {
    format!(
        "MCP servers:\n  Runtime MCP manager status is not attached to command context yet.\n\nConfigured MCP servers cannot be inspected until the runtime manager is attached.\nConfig locations checked by the runtime loader:\n  - ~/.aemeath/mcp.json\n  - {}/.mcp.json",
        ctx.cwd
    )
}

fn format_mcp_tools_from_context(_ctx: &CommandContext, server: Option<&str>) -> String {
    match server {
        Some(name) => format!(
            "MCP tools for '{}':\n  Runtime MCP manager status is not attached to command context yet.",
            name
        ),
        None => "MCP tools:\n  Runtime MCP manager status is not attached to command context yet."
            .to_string(),
    }
}

fn format_mcp_add_preview(name: &str, target: &str, rest: &[&str]) -> String {
    if target.starts_with("http://") || target.starts_with("https://") {
        format!(
            "MCP add requested: server '{}' remote url '{}'.\n  Runtime MCP manager status is not attached to command context yet; add will require the runtime bridge.",
            name, target
        )
    } else {
        let command = if rest.is_empty() {
            target.to_string()
        } else {
            format!("{} {}", target, rest.join(" "))
        };
        format!(
            "MCP add requested: server '{}' command '{}'.\n  Runtime MCP manager status is not attached to command context yet; add will require the runtime bridge.",
            name, command
        )
    }
}

#[cfg(test)]
mod mcp_tests {
    use super::*;

    #[test]
    fn test_format_mcp_add_preview_url() {
        let output = format_mcp_add_preview("remote", "https://example.com/mcp", &[]);

        assert!(output.contains("remote"));
        assert!(output.contains("https://example.com/mcp"));
        assert!(output.contains("remote url"));
    }

    #[test]
    fn test_format_mcp_add_preview_command_with_args() {
        let output = format_mcp_add_preview("local", "/usr/bin/demo", &["--stdio", "--verbose"]);

        assert!(output.contains("local"));
        assert!(output.contains("/usr/bin/demo --stdio --verbose"));
        assert!(output.contains("command"));
    }

    #[test]
    fn test_execute_mcp_command_unknown() {
        let mut ctx = command_context();
        let result = execute_mcp_command("bad", &mut ctx);

        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_execute_mcp_command_list_without_runtime_manager() {
        let mut ctx = command_context();
        let result = execute_mcp_command("", &mut ctx);

        let CommandResult::Success(output) = result else {
            panic!("expected success");
        };
        assert!(output.contains("MCP servers"));
        assert!(output.contains("Runtime MCP manager status is not attached"));
    }

    #[test]
    fn test_execute_mcp_command_tools_without_runtime_manager() {
        let mut ctx = command_context();
        let result = execute_mcp_command("tools local", &mut ctx);

        let CommandResult::Success(output) = result else {
            panic!("expected success");
        };
        assert!(output.contains("MCP tools for 'local'"));
        assert!(output.contains("Runtime MCP manager status is not attached"));
    }

    fn command_context() -> CommandContext {
        CommandContext {
            state: std::sync::Arc::new(crate::state::AppState::new()),
            config: aemeath_core::config::Config::default(),
            cwd: "/tmp/project".to_string(),
            session_id: "test".to_string(),
            verbose: false,
            cost_tracker: crate::cost::CostTracker::new(),
            models_config: aemeath_core::config::ModelsConfig::default(),
            current_model: String::new(),
            task_store: None,
        }
    }
}
