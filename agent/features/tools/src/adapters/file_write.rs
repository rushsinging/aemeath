use crate::domain::types::write::{WriteInput, WriteResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;

fn has_read_evidence(
    ctx: &ToolExecutionContext,
    requested_path: &str,
    resolved_path: &std::path::Path,
) -> bool {
    ctx.read_set().contains(requested_path)
        || ctx
            .read_set()
            .contains(resolved_path.to_string_lossy().as_ref())
}

pub struct FileWriteTool;

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

    async fn call(
        &self,
        input: serde_json::Value,
        ctx: &ToolExecutionContext,
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
        let requested_path = args.file_path.as_str();
        let content = args.content.as_str();
        let path = match ctx.workspace_read().resolve_file_path_authorized(
            std::path::Path::new(requested_path),
            ctx.authorization().allow_outside_workspace,
        ) {
            Ok(path) => path,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let file_path = path.to_string_lossy().into_owned();
        if ctx.authorization().require_read_before_write
            && path.exists()
            && !has_read_evidence(ctx, requested_path, &path)
        {
            return TypedToolResult::error(format!(
                "You must read {} before editing it. Use the Read tool first.",
                path.display()
            ));
        }
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
