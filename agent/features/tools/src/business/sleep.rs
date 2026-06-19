use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    type Result = ToolResult;
    fn name(&self) -> &str {
        "Sleep"
    }
    fn description(&self) -> &str {
        "Pause execution for a specified duration. Useful for waiting for asynchronous operations or rate limiting."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "duration_ms": {
                    "type": "integer",
                    "description": "Duration to sleep in milliseconds (max 60000)",
                    "minimum": 0,
                    "maximum": 60000
                }
            },
            "required": ["duration_ms"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
        let duration_ms = input["duration_ms"].as_u64().unwrap_or(1000);

        // Limit sleep duration to 60 seconds
        let duration_ms = duration_ms.min(60000);

        // Check for cancellation
        if ctx.cancel.is_cancelled() {
            return ToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": "Sleep cancelled",
                    "data": {}
                })
                .to_string(),
            );
        }

        tokio::time::sleep(std::time::Duration::from_millis(duration_ms)).await;

        // Check again after sleep
        if ctx.cancel.is_cancelled() {
            return ToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": "Sleep cancelled",
                    "data": {}
                })
                .to_string(),
            );
        }

        ToolResult::success(
            serde_json::json!({
                "status": "success",
                "message": format!("Slept for {}ms", duration_ms),
                "data": {
                    "duration_ms": duration_ms
                }
            })
            .to_string(),
        )
    }
}
