use crate::command::{Command, CommandCategory, CommandContext, CommandResult};

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
            CommandResult::Success(
                "Available Skills:\n\n  commit - Create a git commit\n  review - Review code changes\n\nUse /skills run <name> to execute a skill".to_string()
            )
        }
        other => {
            CommandResult::Success(format!("Run skill: {} (use Skill tool)", other))
        }
    }
}
