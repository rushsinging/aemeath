use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::tool_search::ToolSearchResult;

/// ToolSearch tool - dynamically searches available tools from the registry.
///
/// Replaces the previous hardcoded tool list. Queries the live `ToolRegistry`
/// via `ToolExecutionContext.registry`, ensuring newly registered tools (e.g.
/// MCP tools) are discoverable.
pub struct ToolSearchTool;

#[async_trait]
impl TypedTool for ToolSearchTool {
    type Output = ToolSearchResult;
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
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        ToolSearchResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<ToolSearchResult> {
        let query = input["query"].as_str().unwrap_or("").to_lowercase();

        // 从注册表动态获取工具列表
        let tools: Vec<(String, String)> = match &ctx.registry {
            Some(reg) => reg
                .tool_names()
                .into_iter()
                .map(|name| {
                    let desc = reg.tool_description(&name).unwrap_or_default();
                    (name, desc)
                })
                .collect(),
            None => Vec::new(),
        };

        if query.is_empty() {
            let tool_names: Vec<&str> = tools.iter().map(|(n, _)| n.as_str()).collect();
            return TypedToolResult::success_msg(
                serde_json::json!({
                    "status": "success",
                    "message": format!("Available tools ({})", tools.len()),
                    "data": {"tools": tool_names}
                })
                .to_string(),
            );
        }

        // 搜索匹配的工具
        let matching: Vec<&(String, String)> = tools
            .iter()
            .filter(|(name, desc)| {
                let name_lower = name.to_lowercase();
                let desc_lower = desc.to_lowercase();
                name_lower.contains(&query)
                    || desc_lower.contains(&query)
                    || match_keywords(&query, name)
            })
            .collect();

        if matching.is_empty() {
            return TypedToolResult::success_msg(
                serde_json::json!({
                    "status": "success",
                    "message": format!("No tools found matching '{}'", query),
                    "data": {"tools": []}
                })
                .to_string(),
            );
        }

        let tool_names: Vec<&str> = matching.iter().map(|(n, _)| n.as_str()).collect();
        TypedToolResult::success_msg(
            serde_json::json!({
                "status": "success",
                "message": format!("Found {} tool(s) matching '{}'", matching.len(), query),
                "data": {"tools": tool_names}
            })
            .to_string(),
        )
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
