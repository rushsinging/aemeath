use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::string_idx::slice_head;
use share::tool::types::lsp::{LspInput, LspResult};
use share::tool::{PathAccess, PathKind};
use tokio::process::Command;

pub struct LspTool;

const FILE_ACCESS: [PathAccess; 1] = [PathAccess {
    field: "filePath",
    kind: PathKind::File,
}];

#[async_trait]
impl TypedTool for LspTool {
    type Output = LspResult;
    fn name(&self) -> &str {
        "LSP"
    }

    fn description(&self) -> &str {
        "Get code intelligence information using language tools.\n\nSupported operations:\n- diagnostics: Get compiler errors/warnings for a file\n- definition: Find the definition of a symbol at a position\n- references: Find all references to a symbol\n- symbols: List symbols in a file or workspace\n\nThis tool uses language-specific CLI tools (cargo, tsc, pylint, etc.) to provide code intelligence."
    }

    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        LspInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        LspResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn path_accesses(&self) -> &'static [PathAccess] {
        &FILE_ACCESS
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<LspResult> {
        let args: LspInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("invalid input: {e}"), "data": null}).to_string()),
        };
        let operation = args.operation.as_str();
        let file_path = args.filePath.as_str();

        let language = args
            .language
            .clone()
            .unwrap_or_else(|| detect_language(file_path));

        // Path has already been validated and normalised by PolicyEngine
        let path_base = ctx.workspace_read().current_path_base();
        let file_path = file_path.to_string();

        match operation {
            "diagnostics" => get_diagnostics(&file_path, &language, &path_base).await,
            "symbols" => get_symbols(&file_path, &language, &path_base).await,
            _ => TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("unsupported operation: {operation}"), "data": null}).to_string()),
        }
    }
}

fn detect_language(file_path: &str) -> String {
    let ext = file_path.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "rb" => "ruby",
        _ => "unknown",
    }
    .to_string()
}

async fn get_diagnostics(
    file_path: &str,
    language: &str,
    cwd: &std::path::Path,
) -> TypedToolResult<LspResult> {
    let output = match language {
        "rust" => {
            Command::new("cargo")
                .args(["check", "--message-format=short"])
                .current_dir(cwd)
                .output()
                .await
        }
        "typescript" | "javascript" => {
            Command::new("npx")
                .args(["tsc", "--noEmit", "--pretty", "false"])
                .current_dir(cwd)
                .output()
                .await
        }
        "python" => {
            Command::new("python3")
                .args(["-m", "py_compile", file_path])
                .current_dir(cwd)
                .output()
                .await
        }
        "go" => {
            Command::new("go")
                .args(["vet", "./..."])
                .current_dir(cwd)
                .output()
                .await
        }
        _ => {
            return TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("diagnostics not supported for language: {language}"), "data": null}).to_string());
        }
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = if !stderr.is_empty() && !stdout.is_empty() {
                format!("{stdout}\n{stderr}")
            } else if !stderr.is_empty() {
                stderr.to_string()
            } else if !stdout.is_empty() {
                stdout.to_string()
            } else {
                "no diagnostics (clean)".to_string()
            };

            // Truncate very long output
            if combined.len() > 10000 {
                let truncated_output = format!("{}...\n[truncated]", slice_head(&combined, 10000));
                TypedToolResult::success(
                    truncated_output.clone(),
                    LspResult { output: truncated_output },
                )
            } else {
                TypedToolResult::success(
                    combined.clone(),
                    LspResult { output: combined },
                )
            }
        }
        Err(e) => TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("failed to run diagnostics: {e}"), "data": null}).to_string()),
    }
}

async fn get_symbols(
    file_path: &str,
    language: &str,
    cwd: &std::path::Path,
) -> TypedToolResult<LspResult> {
    let output = match language {
        "rust" => {
            // Use grep to find fn/struct/enum/impl/trait/mod definitions
            Command::new("grep")
                .args([
                    "-n",
                    "-E",
                    r"^\s*(pub\s+)?(fn|struct|enum|impl|trait|mod|type|const|static)\s+",
                    file_path,
                ])
                .current_dir(cwd)
                .output()
                .await
        }
        "typescript" | "javascript" => {
            Command::new("grep")
                .args([
                    "-n",
                    "-E",
                    r"^\s*(export\s+)?(function|class|interface|type|enum|const|let|var)\s+",
                    file_path,
                ])
                .current_dir(cwd)
                .output()
                .await
        }
        "python" => {
            Command::new("grep")
                .args(["-n", "-E", r"^(class|def|async def)\s+", file_path])
                .current_dir(cwd)
                .output()
                .await
        }
        "go" => {
            Command::new("grep")
                .args(["-n", "-E", r"^(func|type|var|const)\s+", file_path])
                .current_dir(cwd)
                .output()
                .await
        }
        _ => {
            // Fallback: extract lines that look like declarations
            Command::new("grep")
                .args([
                    "-n",
                    "-E",
                    r"^\s*(pub|public|export|def|fn|func|function|class|struct|enum|interface|type|trait|impl|const|static|let|var)\s+",
                    file_path,
                ])
                .current_dir(cwd)
                .output()
                .await
        }
    };

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.is_empty() {
                TypedToolResult::success(
                    "no symbols found",
                    LspResult { output: String::new() },
                )
            } else {
                TypedToolResult::success(
                    stdout.to_string(),
                    LspResult { output: stdout.to_string() },
                )
            }
        }
        Err(e) => TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("failed to get symbols: {e}"), "data": null}).to_string()),
    }
}
