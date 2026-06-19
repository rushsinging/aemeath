use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

/// ToolSearch tool - searches for available tools.
/// Note: This tool provides a static list of known tools since the registry
/// cannot be cloned. The actual availability depends on what's registered.
pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearch"
    }
    fn description(&self) -> &str {
        "Search for available tools by name or functionality. Use this to discover tools that can help with specific tasks."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query - tool name or functionality keyword"
                }
            },
            "required": ["query"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: serde_json::Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let query = input["query"].as_str().unwrap_or("").to_lowercase();

        // 预定义的工具列表及其描述
        let all_tools = [
            ("Bash", "Execute bash commands with timeout support"),
            ("Read", "Read file contents (alias: FileRead)"),
            ("Write", "Write file contents (alias: FileWrite)"),
            (
                "Edit",
                "Edit file with exact string replacement (alias: FileEdit)",
            ),
            ("Glob", "Fast file pattern matching tool"),
            ("Grep", "Search file contents with regex support"),
            (
                "LSP",
                "Language server protocol operations (diagnostics, definitions, references)",
            ),
            ("WebFetch", "Fetch content from URLs"),
            ("WebSearch", "Search the web for information"),
            ("Agent", "Launch a sub-agent for complex multi-step tasks"),
            ("TaskCreate", "Create a task to track multi-step work"),
            ("TaskUpdate", "Update task status, subject, or dependencies"),
            ("TaskList", "List all tasks and their status"),
            ("TaskGet", "Retrieve a specific task by ID"),
            ("TaskStop", "Stop a running or pending task"),
            ("MCP", "Call MCP server tools"),
            ("Skill", "Execute a skill template"),
            ("Config", "View or modify configuration settings"),
            ("Sleep", "Pause execution for a specified duration"),
            ("AskUserQuestion", "Ask user for input or confirmation"),
            ("ToolSearch", "Search for available tools"),
        ];

        if query.is_empty() {
            // Return all available tools
            return ToolResult::success(serde_json::json!({"status": "success", "message": format!("Available tools ({})", all_tools.len()), "data": {"tools": all_tools.iter().map(|(name, desc)| serde_json::json!({"name": name, "description": desc})).collect::<Vec<_>>()}}).to_string());
        }

        // Search for matching tools
        let matching_tools: Vec<(&str, &str)> = all_tools
            .iter()
            .filter(|(name, desc)| {
                let name_lower = name.to_lowercase();
                let desc_lower = desc.to_lowercase();
                name_lower.contains(&query)
                    || desc_lower.contains(&query)
                    || match_keywords(&query, name)
            })
            .copied()
            .collect();

        if matching_tools.is_empty() {
            return ToolResult::success(serde_json::json!({"status": "success", "message": format!("No tools found matching '{}'", query), "data": {"query": query, "tools": [], "hint": "Use ToolSearch with empty query to see all available tools."}}).to_string());
        }

        ToolResult::success(serde_json::json!({"status": "success", "message": format!("Found {} tool(s) matching '{}'", matching_tools.len(), query), "data": {"query": query, "tools": matching_tools.iter().map(|(name, desc)| serde_json::json!({"name": name, "description": desc})).collect::<Vec<_>>()}}).to_string())
    }
}

fn match_keywords(query: &str, tool_name: &str) -> bool {
    let tool_lower = tool_name.to_lowercase();

    // 关键词映射
    let keyword_mappings = [
        ("file", vec!["file", "read", "write", "edit"]),
        ("search", vec!["grep", "glob", "search", "find"]),
        ("run", vec!["bash", "shell", "execute"]),
        ("task", vec!["task", "todo"]),
        ("web", vec!["web", "fetch", "search"]),
        ("agent", vec!["agent", "subagent"]),
        ("mcp", vec!["mcp"]),
        ("skill", vec!["skill"]),
        ("config", vec!["config", "settings"]),
        ("ask", vec!["ask", "question", "user"]),
        ("sleep", vec!["sleep", "wait", "delay"]),
        ("lsp", vec!["lsp", "language", "intellisense"]),
    ];

    for (keyword, tools) in keyword_mappings {
        if query.contains(keyword) || keyword.contains(query) {
            for t in tools {
                if tool_lower.contains(t) {
                    return true;
                }
            }
        }
    }

    false
}
