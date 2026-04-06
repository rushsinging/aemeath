use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;
use tokio::process::Command;

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        "Fetches content from a specified URL via HTTP GET.\n\nUsage:\n- The URL must be a fully-formed valid URL\n- HTTP URLs will be automatically upgraded to HTTPS\n- This tool is read-only and does not modify any files\n- Results may be truncated if the content is very large\n- For GitHub URLs, prefer using the gh CLI via Bash instead (e.g., gh pr view, gh issue view)"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default 30000)"
                }
            },
            "required": ["url"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let url = match input.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return ToolResult::error("missing required parameter: url"),
        };

        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30_000);

        // Use curl as it's universally available and handles redirects/TLS
        let result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            Command::new("curl")
                .args([
                    "-sL",           // silent, follow redirects
                    "--max-time", &(timeout_ms / 1000).max(5).to_string(),
                    "-A", "aemeath/0.1.0",
                    url,
                ])
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let body = String::from_utf8_lossy(&output.stdout);
                    // Truncate very large responses
                    let max_chars = 50_000;
                    if body.len() > max_chars {
                        ToolResult::success(format!(
                            "{}...\n\n[truncated, showing first {} chars of {} total]",
                            &body[..max_chars],
                            max_chars,
                            body.len()
                        ))
                    } else {
                        ToolResult::success(body.to_string())
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    ToolResult::error(format!("fetch failed: {stderr}"))
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("failed to execute curl: {e}")),
            Err(_) => ToolResult::error(format!("request timed out after {timeout_ms}ms")),
        }
    }
}
