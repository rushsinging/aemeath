use aemeath_core::mcp::McpClient;
use aemeath_core::tool::{Tool, ToolContext, ToolResult};
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
impl Tool for McpTool {
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
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let client = self.client.lock().await;
        match client.call_tool(&self.tool_name, input).await {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("MCP tool error: {e}")),
        }
    }
}
