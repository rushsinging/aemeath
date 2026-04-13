use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Normalize and validate a file path against the workspace boundary.
/// Returns the normalized path if valid, or an error message if it escapes the workspace.
fn validate_and_normalize_path(file_path: &str, workspace_root: &Path) -> Result<PathBuf, String> {
    // Convert to absolute path
    let abs_path = if Path::new(file_path).is_absolute() {
        PathBuf::from(file_path)
    } else {
        workspace_root.join(file_path)
    };
    
    // Normalize the path (resolve .., symlinks, etc.)
    let normalized = abs_path.canonicalize()
        .or_else(|_| {
            // If canonicalize fails (e.g., file doesn't exist yet), use parent resolution
            let parent = abs_path.parent().unwrap_or(&abs_path);
            parent.canonicalize().map(|p| p.join(abs_path.file_name().unwrap_or_default()))
        })
        .unwrap_or_else(|_| abs_path);
    
    // Convert workspace root to absolute and normalize
    let workspace_abs = workspace_root.canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    
    // Check that the normalized path is within the workspace
    let normalized_str = normalized.to_string_lossy();
    let workspace_str = workspace_abs.to_string_lossy();
    
    if !normalized_str.starts_with(&*workspace_str) {
        return Err(format!(
            "Path '{}' escapes workspace '{}'. Only files within the workspace are allowed.",
            normalized_str, workspace_str
        ));
    }
    
    Ok(normalized)
}

/// Check if a path attempts directory traversal
fn has_traversal_attempt(file_path: &str) -> bool {
    file_path.contains("..")
}

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
            None => return ToolResult::error("missing required parameter: file_path"),
        };
        let content = match input.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("missing required parameter: content"),
        };
        // Check for directory traversal attempts
        if has_traversal_attempt(file_path) {
            return ToolResult::error(format!(
                "Directory traversal attempt detected: {}\nOnly files within the workspace are allowed.",
                file_path
            ));
        }

        // Validate path is within workspace boundary
        let path = match validate_and_normalize_path(file_path, &ctx.cwd) {
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
