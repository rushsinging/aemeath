use crate::domain::types::write::{WriteInput, WriteResult};
use crate::domain::{PathAccess, PathKind};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;

pub struct FileWriteTool;

const FILE_ACCESS: [PathAccess; 1] = [PathAccess {
    field: "file_path",
    kind: PathKind::File,
}];

#[async_trait]
impl TypedTool for FileWriteTool {
    type Output = WriteResult;
    fn name(&self) -> &str {
        "Write"
    }
    fn description(&self) -> &str {
        "Writes a file to the local filesystem. Requires `file_path` and `content`. For existing files, Read must be called first. Prefer Edit for modifications; use Write for new files or complete rewrites."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::filesystem::file_write(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        WriteInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        WriteResult::data_schema()
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

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<WriteResult> {
        let received_keys = input
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        let args: WriteInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!(
                            "invalid input ({e}). Write takes both `file_path` (absolute or workspace-relative path) and `content` (string). Received keys: [{received_keys}]"
                        ),
                        "data": {}
                    })
                    .to_string(),
                )
            }
        };
        let file_path = args.file_path.as_str();
        let content = args.content.as_str();
        // Path has already been validated and normalised by PolicyEngine,
        // including the read-before-write check for existing files.
        let path = std::path::PathBuf::from(file_path);
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return TypedToolResult::error(
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
            Ok(()) => {
                let data = WriteResult {
                    file_path: file_path.to_string(),
                    bytes_written: content.len() as u64,
                };
                TypedToolResult::success(
                    format!("Wrote {} bytes to {file_path}", content.len()),
                    data,
                )
            }
            Err(e) => TypedToolResult::error(
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
