use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::sleep::{SleepInput, SleepResult};

pub struct SleepTool;

#[async_trait]
impl TypedTool for SleepTool {
    type Output = SleepResult;
    fn name(&self) -> &str {
        "Sleep"
    }
    fn description(&self) -> &str {
        "Pause execution for a specified duration. Useful for waiting for asynchronous operations or rate limiting."
    }
    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        SleepInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        SleepResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<SleepResult> {
        let args: SleepInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let duration_ms = args.duration_ms;

        // Limit sleep duration to 60 seconds
        let duration_ms = duration_ms.min(60000);

        // Check for cancellation
        if ctx.cancel.is_cancelled() {
            return TypedToolResult::error(
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
            return TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": "Sleep cancelled",
                    "data": {}
                })
                .to_string(),
            );
        }

        TypedToolResult::success_msg(
            serde_json::json!({
                "status": "success",
                "message": format!("Slept for {}ms", duration_ms),
                "data": serde_json::to_value(SleepResult { duration_ms }).unwrap()
            })
            .to_string(),
        )
    }
}
