use crate::api::{Tool, ToolExecutionContext, ToolResult};
use crate::business::mcp::McpClient;
use crate::business::mcp_manager::McpConnectionManager;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) struct McpToolWrapper {
    pub(crate) tool_name: String,
    pub(crate) qualified_name: String,
    pub(crate) description: String,
    pub(crate) schema: Value,
    pub(crate) client: Arc<Mutex<McpClient>>,
}

/// Validate MCP tool input against JSON Schema
fn validate_mcp_input(input: &Value, schema: &Value) -> Result<(), String> {
    // Basic schema validation - check required fields
    if let Some(obj) = input.as_object() {
        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            // Check required fields
            if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
                for field in required {
                    if let Some(field_name) = field.as_str() {
                        if !obj.contains_key(field_name) {
                            return Err(format!("Missing required field: {}", field_name));
                        }
                    }
                }
            }

            // Check field types
            for (key, value) in obj {
                if let Some(prop_schema) = props.get(key) {
                    let expected_type = prop_schema
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("any");
                    let actual_type = match value {
                        Value::Null => "null",
                        Value::Bool(_) => "boolean",
                        Value::Number(_) => "number",
                        Value::String(_) => "string",
                        Value::Array(_) => "array",
                        Value::Object(_) => "object",
                    };
                    // Allow number to match integer type loosely
                    if expected_type != "any"
                        && expected_type != actual_type
                        && !(expected_type == "integer" && actual_type == "number")
                    {
                        return Err(format!(
                            "Type mismatch for field '{}': expected {}, got {}",
                            key, expected_type, actual_type
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    fn is_read_only(&self) -> bool {
        // MCP tools are generally not read-only unless specified
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        // MCP tools are generally concurrency safe
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolExecutionContext) -> ToolResult {
        // Validate input against schema before calling MCP tool
        if let Err(e) = validate_mcp_input(&input, &self.schema) {
            log::warn!("MCP tool {} input validation failed: {}", self.tool_name, e);
            return ToolResult::error(format!("Invalid input for {}: {}", self.tool_name, e));
        }

        let client = self.client.lock().await;
        match client.call_tool(&self.tool_name, input).await {
            Ok(output) => ToolResult::success(crate::business::mcp::limit_tool_response(
                &output,
                crate::business::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES,
            )),
            Err(e) => ToolResult::error(format!("MCP tool error: {}", e)),
        }
    }
}

/// Shared MCP connection manager
pub type SharedMcpManager = Arc<McpConnectionManager>;
