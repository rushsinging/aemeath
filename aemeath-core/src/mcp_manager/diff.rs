use crate::mcp::McpToolDef;
use std::collections::HashMap;

pub struct ToolListDiff {
    /// Tools present in the new list but absent from the old list.
    pub added: Vec<McpToolDef>,
    /// Tool names present in the old list but absent from the new list.
    pub removed: Vec<String>,
    /// Tools whose description or input schema changed.
    pub changed: Vec<McpToolDef>,
}

/// Compute added, removed, and changed MCP tools by tool name.
pub fn diff_tools(old: &[McpToolDef], new: &[McpToolDef]) -> ToolListDiff {
    let old_by_name: HashMap<&str, &McpToolDef> =
        old.iter().map(|tool| (tool.name.as_str(), tool)).collect();
    let new_by_name: HashMap<&str, &McpToolDef> =
        new.iter().map(|tool| (tool.name.as_str(), tool)).collect();

    let added = new
        .iter()
        .filter(|tool| !old_by_name.contains_key(tool.name.as_str()))
        .cloned()
        .collect();

    let removed = old
        .iter()
        .filter(|tool| !new_by_name.contains_key(tool.name.as_str()))
        .map(|tool| tool.name.clone())
        .collect();

    let changed = new
        .iter()
        .filter(|new_tool| {
            old_by_name
                .get(new_tool.name.as_str())
                .is_some_and(|old_tool| {
                    old_tool.description != new_tool.description
                        || old_tool.input_schema != new_tool.input_schema
                })
        })
        .cloned()
        .collect();

    ToolListDiff {
        added,
        removed,
        changed,
    }
}

/// Build the registry-qualified name for an MCP tool.
pub fn qualified_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", server, tool)
}

/// Build registry-qualified names for tools removed from an MCP server.
pub fn removed_qualified_tool_names(server: &str, removed: &[String]) -> Vec<String> {
    removed
        .iter()
        .map(|tool| qualified_tool_name(server, tool))
        .collect()
}
