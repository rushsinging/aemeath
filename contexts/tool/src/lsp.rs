use crate::path_security::validate_and_normalize_path_from_base;
use async_trait::async_trait;
use kernel::compact::safe_slice;
use kernel::tool::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use tokio::process::Command;

pub struct LspTool;

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "LSP"
    }

    fn description(&self) -> &str {
        "Get code intelligence information using language tools.\n\nSupported operations:\n- diagnostics: Get compiler errors/warnings for a file\n- definition: Find the definition of a symbol at a position\n- references: Find all references to a symbol\n- symbols: List symbols in a file or workspace\n\nThis tool uses language-specific CLI tools (cargo, tsc, pylint, etc.) to provide code intelligence."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["diagnostics", "symbols"],
                    "description": "The LSP operation to perform"
                },
                "filePath": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "language": {
                    "type": "string",
                    "description": "Language hint (rust, typescript, python, go). Auto-detected from file extension if omitted."
                }
            },
            "required": ["operation", "filePath"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let operation = match input.get("operation").and_then(|v| v.as_str()) {
            Some(op) => op,
            None => return ToolResult::error("missing required parameter: operation"),
        };

        let file_path = match input.get("filePath").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::error("missing required parameter: filePath"),
        };

        let language = input
            .get("language")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| detect_language(file_path));

        let path_base = ctx.current_path_base();
        let working_root = ctx.current_working_root();
        let file_path = match validate_and_normalize_path_from_base(
            file_path,
            &path_base,
            &working_root,
            ctx.allow_all,
        ) {
            Ok(path) => path,
            Err(e) => return ToolResult::error(e),
        };
        let file_path = file_path.to_string_lossy().to_string();

        match operation {
            "diagnostics" => get_diagnostics(&file_path, &language, &path_base).await,
            "symbols" => get_symbols(&file_path, &language, &path_base).await,
            _ => ToolResult::error(format!("unsupported operation: {operation}")),
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

async fn get_diagnostics(file_path: &str, language: &str, cwd: &std::path::Path) -> ToolResult {
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
            return ToolResult::error(format!(
                "diagnostics not supported for language: {language}"
            ));
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
                ToolResult::success(format!("{}...\n[truncated]", safe_slice(&combined, 10000)))
            } else {
                ToolResult::success(combined)
            }
        }
        Err(e) => ToolResult::error(format!("failed to run diagnostics: {e}")),
    }
}

async fn get_symbols(file_path: &str, language: &str, cwd: &std::path::Path) -> ToolResult {
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
                ToolResult::success("no symbols found")
            } else {
                ToolResult::success(stdout.to_string())
            }
        }
        Err(e) => ToolResult::error(format!("failed to get symbols: {e}")),
    }
}
