use crate::adapters::mcp::McpClient;
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A dynamically created tool that proxies calls to an MCP server
pub struct McpTool {
    pub tool_name: String,
    pub qualified_name: String,
    pub tool_description: String,
    pub schema: Value,
    pub client: Arc<Mutex<McpClient>>,
}

#[async_trait]
impl TypedTool for McpTool {
    type Output = Value;

    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        // MCP tool side effects are server-defined; keep serial until capabilities say otherwise.
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let client = self.client.lock().await;
        match client.call_tool(&self.tool_name, input).await {
            Ok(output) => {
                let limited = crate::adapters::mcp::limit_tool_response(
                    &output,
                    crate::adapters::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES,
                );
                let data =
                    serde_json::from_str::<Value>(&limited).unwrap_or(Value::String(limited));
                TypedToolResult::success("MCP tool call succeeded", data)
            }
            Err(e) => TypedToolResult::error(format!("MCP tool error: {e}")),
        }
    }
}
