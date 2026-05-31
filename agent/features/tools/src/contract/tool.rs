use super::ToolContext;
use async_trait::async_trait;
use serde_json::Value;
use share::tool::ToolResult;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    /// Timeout for this tool in seconds (default 120s, override for long-running tools)
    fn timeout_secs(&self) -> u64 {
        120
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult;
}
