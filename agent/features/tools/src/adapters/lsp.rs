use crate::domain::types::lsp::{LspInput, LspResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::string_idx::slice_head;
use tokio::process::Command;

pub struct LspTool;

#[async_trait]
impl TypedTool for LspTool {
    type Output = LspResult;
    fn name(&self) -> &str {
        "LSP"
    }

    fn description(&self) -> &str {
        "Get code intelligence information using language tools.\n\nSupported operations:\n- diagnostics: Get compiler errors/warnings for a file\n- definition: Find the definition of a symbol at a position\n- references: Find all references to a symbol\n- symbols: List symbols in a file or workspace\n\nThis tool uses language-specific CLI tools (cargo, tsc, pylint, etc.) to provide code intelligence."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::lsp(lang))
    }

    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        LspInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        LspResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        // Conservative default: diagnostics may run cargo/tsc/go and write build caches.
        false
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<LspResult> {
        let args: LspInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("invalid input: {e}"), "data": null}).to_string()),
        };
        let operation = args.operation.as_str();
        let requested_path = args.file_path.as_str();

        let language = args
            .language
            .clone()
            .unwrap_or_else(|| detect_language(requested_path));

        let workspace = ctx.workspace_read();
        let path = match workspace.resolve_file_path_authorized(
            std::path::Path::new(requested_path),
            ctx.authorization().allow_outside_workspace,
        ) {
            Ok(path) => path,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let path_base = workspace.current_path_base();
        let file_path = path.to_string_lossy().into_owned();

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
