use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::{PathAccess, PathKind};

pub struct FileWriteTool;

const FILE_ACCESS: [PathAccess; 1] = [PathAccess {
    field: "file_path",
    kind: PathKind::File,
}];

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "Write"
    }
    fn description(&self) -> &str {
        "Writes a file to the local filesystem.\n\nRequired arguments (BOTH must be present):\n- `file_path`: absolute or workspace-relative path of the file to create or overwrite\n- `content`: the full text to write into that file\n\nExample call:\n  Write({\"file_path\": \"src/foo.rs\", \"content\": \"fn main() {}\\n\"})\n\nRules:\n- Overwrites the existing file at `file_path` if it exists.\n- For existing files you MUST call Read first; this tool fails otherwise.\n- Prefer the Edit tool for modifying existing files — it only sends the diff. Use Write only for new files or complete rewrites.\n- NEVER create documentation files (*.md) or README files unless explicitly requested by the user.\n- DO NOT call Write with empty arguments — both `file_path` and `content` are required strings."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute or workspace-relative path of the file to create or overwrite. Required.",
                    "minLength": 1
                },
                "content": {
                    "type": "string",
                    "description": "Full text content to write into the file. Required (use empty string for an empty file)."
                }
            },
            "required": ["file_path", "content"],
            "additionalProperties": false
        })
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }
    fn path_accesses(&self) -> &'static [PathAccess] {
        &FILE_ACCESS
    }
    fn requires_read_before_write(&self) -> bool {
        true
    }

    async fn call(&self, input: serde_json::Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let file_path = match input.get("file_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error(serde_json::json!({
                "status": "error",
                "message": format!(
                    "missing required parameter: file_path. Write takes both `file_path` (absolute or workspace-relative path) and `content` (string). Received keys: [{}]",
                    input.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default()
                ),
                "data": {}
            }).to_string()),
        };
        let content = match input.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error(serde_json::json!({
                "status": "error",
                "message": format!(
                    "missing required parameter: content. Write takes both `file_path` and `content` (string). Received keys: [{}]",
                    input.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default()
                ),
                "data": {}
            }).to_string()),
        };
        // Path has already been validated and normalised by PolicyEngine,
        // including the read-before-write check for existing files.
        let path = std::path::PathBuf::from(file_path);
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return ToolResult::error(
                        serde_json::json!({
                            "status": "error",
                            "message": format!("failed to create directory: {e}"),
                            "data": {
                                "file_path": file_path
                            }
                        })
                        .to_string(),
                    );
                }
            }
        }
        match tokio::fs::write(path, content).await {
            Ok(()) => ToolResult::success(
                serde_json::json!({
                    "status": "success",
                    "message": format!("Wrote {} bytes to {file_path}", content.len()),
                    "data": {
                        "file_path": file_path,
                        "bytes_written": content.len()
                    }
                })
                .to_string(),
            ),
            Err(e) => ToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("failed to write file: {e}"),
                    "data": {
                        "file_path": file_path
                    }
                })
                .to_string(),
            ),
        }
    }
}
