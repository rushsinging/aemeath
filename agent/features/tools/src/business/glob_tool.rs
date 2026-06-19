use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::glob::GlobResult;
use share::tool::{PathAccess, PathKind};

pub struct GlobTool;

const PATH_ACCESS: [PathAccess; 1] = [PathAccess {
    field: "path",
    kind: PathKind::SearchDir,
}];

#[async_trait]
impl TypedTool for GlobTool {
    type Output = GlobResult;
    fn name(&self) -> &str {
        "Glob"
    }
    fn description(&self) -> &str {
        "Fast file pattern matching tool. Supports glob patterns (e.g. \"**/*.rs\"). Returns paths sorted by modification time."
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
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        GlobResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn path_accesses(&self) -> &'static [PathAccess] {
        &PATH_ACCESS
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<GlobResult> {
        let pattern = match input.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": "missing required parameter: pattern",
                        "data": {}
                    })
                    .to_string(),
                )
            }
        };
        // Path has already been validated and normalised by PolicyEngine
        let base_dir = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                // Fallback to cwd if path is missing
                _ctx.workspace_read().current_path_base()
            }
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
                    TypedToolResult::success_msg(
                        serde_json::json!({
                            "status": "success",
                            "message": "No files matched",
                            "data": serde_json::to_value(GlobResult { files: vec![], count: 0 }).unwrap()
                        })
                        .to_string(),
                    )
                } else {
                    let count = matches.len();
                    let message = format!("Found {count} files");
                    TypedToolResult::success_msg(
                        serde_json::json!({
                            "status": "success",
                            "message": message,
                            "data": serde_json::to_value(GlobResult { files: matches.clone(), count: matches.len() as u64 }).unwrap()
                        })
                        .to_string(),
                    )
                }
            }
            Err(e) => TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("invalid glob pattern: {e}"),
                    "data": {}
                })
                .to_string(),
            ),
        }
    }
}
