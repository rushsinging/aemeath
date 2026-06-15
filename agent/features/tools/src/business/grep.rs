use crate::api::{Tool, ToolExecutionContext, ToolResult};
use crate::utils::path_security::validate_search_path_from_base;
use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }
    fn description(&self) -> &str {
        "A powerful search tool built on ripgrep.\n\nUsage:\n- ALWAYS use Grep for search tasks. NEVER invoke `grep` or `rg` as a Bash command.\n- Supports full regex syntax (e.g., \"log.*Error\", \"function\\\\s+\\\\w+\")\n- Filter files with glob parameter (e.g., \"*.js\", \"**/*.tsx\")\n- Pattern syntax: Uses ripgrep (not grep) - literal braces need escaping\n- Use Agent tool for open-ended searches requiring multiple rounds"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" },
                "path": { "type": "string", "description": "File or directory to search in (defaults to cwd)" },
                "glob": { "type": "string", "description": "File glob filter (e.g. \"*.rs\")" }
            },
            "required": ["pattern"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
        let pattern = match input.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": "Missing required parameter: pattern",
                        "data": {
                            "matches": [],
                            "match_count": 0
                        }
                    })
                    .to_string(),
                )
            }
        };
        let path_str = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let path_base = ctx.workspace_read().current_path_base();
        let working_root = ctx.workspace_read().current_root();
        let search_path = match validate_search_path_from_base(path_str, &path_base, &working_root)
        {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": e,
                        "data": {
                            "matches": [],
                            "match_count": 0
                        }
                    })
                    .to_string(),
                )
            }
        };
        let glob_filter = input.get("glob").and_then(|v| v.as_str());

        let output = if is_rg_available().await {
            let mut cmd = Command::new("rg");
            cmd.arg("-n").arg("--no-heading").arg(pattern);
            if let Some(g) = glob_filter {
                cmd.arg("--glob").arg(g);
            }
            cmd.arg(&search_path)
                .current_dir(&working_root)
                .output()
                .await
        } else {
            let mut cmd = Command::new("grep");
            cmd.arg("-rn").arg(pattern).arg(&search_path);
            if let Some(g) = glob_filter {
                cmd.arg("--include").arg(g);
            }
            cmd.current_dir(&working_root).output().await
        };

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.is_empty() {
                    ToolResult::success(
                        serde_json::json!({
                            "status": "success",
                            "message": "No matches found",
                            "data": {
                                "matches": [],
                                "match_count": 0
                            }
                        })
                        .to_string(),
                    )
                } else {
                    let lines: Vec<&str> = stdout.lines().take(250).collect();
                    let match_count = lines.len();
                    ToolResult::success(
                        serde_json::json!({
                            "status": "success",
                            "message": format!("Found {} matches", match_count),
                            "data": {
                                "matches": lines,
                                "match_count": match_count
                            }
                        })
                        .to_string(),
                    )
                }
            }
            Err(e) => ToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("Search failed: {e}"),
                    "data": {
                        "matches": [],
                        "match_count": 0
                    }
                })
                .to_string(),
            ),
        }
    }
}

async fn is_rg_available() -> bool {
    Command::new("rg")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}
