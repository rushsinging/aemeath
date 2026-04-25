use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use crate::path_security::validate_and_normalize_path;
use serde_json::Value;

pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str { "Write" }
    fn description(&self) -> &str {
        "Writes a file to the local filesystem.\n\nUsage:\n- This tool will overwrite the existing file if there is one at the provided path.\n- If this is an existing file, you MUST use the Read tool first to read the file's contents. This tool will fail if you did not read the file first.\n- Prefer the Edit tool for modifying existing files -- it only sends the diff. Only use this tool to create new files or for complete rewrites.\n- NEVER create documentation files (*.md) or README files unless explicitly requested by the User."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Absolute path to the file to write" },
                "content": { "type": "string", "description": "The content to write" }
            },
            "required": ["file_path", "content"]
        })
    }
    fn is_concurrency_safe(&self) -> bool { false }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let file_path = match input.get("file_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error(format!(
                "missing required parameter: file_path. Write takes both `file_path` (absolute or workspace-relative path) and `content` (string). Received keys: [{}]",
                input.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default()
            )),
        };
        let content = match input.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error(format!(
                "missing required parameter: content. Write takes both `file_path` and `content` (string). Received keys: [{}]",
                input.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default()
            )),
        };
        // Validate path is within workspace boundary
        let path = match validate_and_normalize_path(file_path, &ctx.cwd, ctx.allow_all) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(e),
        };

        // For existing files, require read first
        if path.exists() {
            if let Ok(read_files) = ctx.read_files.lock() {
                if !read_files.contains(file_path) {
                    return ToolResult::error(format!(
                        "You must read {file_path} before overwriting it. Use the Read tool first."
                    ));
                }
            }
        }
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return ToolResult::error(format!("failed to create directory: {e}"));
                }
            }
        }
        match tokio::fs::write(path, content).await {
            Ok(()) => ToolResult::success(format!("wrote {} bytes to {file_path}", content.len())),
            Err(e) => ToolResult::error(format!("failed to write file: {e}")),
        }
    }
}
