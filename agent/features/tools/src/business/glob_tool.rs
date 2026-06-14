use crate::api::{Tool, ToolExecutionContext, ToolResult};
use crate::utils::path_security::validate_search_path_from_base;
use async_trait::async_trait;
use serde_json::Value;

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }
    fn description(&self) -> &str {
        "Fast file pattern matching tool that works with any codebase size.\n- Supports glob patterns like \"**/*.js\" or \"src/**/*.ts\"\n- Returns matching file paths sorted by modification time\n- Use this tool when you need to find files by name patterns\n- When you are doing an open ended search that may require multiple rounds of globbing and grepping, use the Agent tool instead"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern (e.g. \"**/*.rs\")" },
                "path": { "type": "string", "description": "Directory to search in (defaults to cwd)" }
            },
            "required": ["pattern"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
        let pattern = match input.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error(serde_json::json!({
                "status": "error",
                "message": "missing required parameter: pattern",
                "data": {}
            }).to_string()),
        };
        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let path_base = ctx.workspace_read().current_path_base();
        let working_root = ctx.workspace_read().current_root();
        let base_dir = match validate_search_path_from_base(path_str, &path_base, &working_root) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(serde_json::json!({
                "status": "error",
                "message": format!("{e}"),
                "data": {}
            }).to_string()),
        };
        let full_pattern = base_dir.join(pattern).to_string_lossy().to_string();
        match glob::glob(&full_pattern) {
            Ok(paths) => {
                let mut matches: Vec<String> = paths
                    .filter_map(|e| e.ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                matches.sort();
                if matches.is_empty() {
                    ToolResult::success(serde_json::json!({
                        "status": "success",
                        "message": "No files matched",
                        "data": { "files": [], "count": 0 }
                    }).to_string())
                } else {
                    let count = matches.len();
                    let message = format!("Found {count} files");
                    ToolResult::success(serde_json::json!({
                        "status": "success",
                        "message": message,
                        "data": { "files": matches, "count": count }
                    }).to_string())
                }
            }
            Err(e) => ToolResult::error(serde_json::json!({
                "status": "error",
                "message": format!("invalid glob pattern: {e}"),
                "data": {}
            }).to_string()),
        }
    }
}
