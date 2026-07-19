use crate::domain::types::glob::{GlobInput, GlobResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;

pub struct GlobTool;

#[async_trait]
impl TypedTool for GlobTool {
    type Output = GlobResult;
    fn name(&self) -> &str {
        "Glob"
    }
    fn description(&self) -> &str {
        "Fast file pattern matching tool. Supports glob patterns (e.g. \"**/*.rs\"). Returns paths sorted by modification time."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::filesystem::glob(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        GlobInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        GlobResult::data_schema()
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
    ) -> TypedToolResult<GlobResult> {
        let args: GlobInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!("invalid input: {e}"),
                        "data": {}
                    })
                    .to_string(),
                )
            }
        };
        let pattern = args.pattern.as_str();
        let base_dir = match args.path.as_deref() {
            Some(path) => match ctx.workspace_read().resolve_search_path_authorized(
                std::path::Path::new(path),
                ctx.authorization().allow_outside_workspace,
            ) {
                Ok(path) => path,
                Err(error) => return TypedToolResult::error(error.to_string()),
            },
            None => ctx.workspace_read().current_path_base(),
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
                    TypedToolResult::success(
                        "No files matched",
                        GlobResult {
                            files: vec![],
                            count: 0,
                        },
                    )
                } else {
                    let count = matches.len();
                    let list = matches.join("\n");
                    TypedToolResult::success(
                        format!("Found {count} files\n{list}"),
                        GlobResult {
                            files: matches.clone(),
                            count: count as u64,
                        },
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
