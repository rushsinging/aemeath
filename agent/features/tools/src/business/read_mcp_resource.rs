//! Read a specific MCP resource by URI

use crate::api::{Tool, ToolExecutionContext, ToolResult};
use crate::business::mcp::McpClient;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Read MCP resource tool
pub struct ReadMcpResourceTool {
    pub clients: Arc<Mutex<Vec<Arc<Mutex<McpClient>>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    /// Resource URI
    pub uri: String,
    /// MIME type of the content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Text content of the resource
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Path where binary blob content was saved
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_saved_to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadMcpResourceOutput {
    pub contents: Vec<ResourceContent>,
}

#[async_trait]
impl Tool for ReadMcpResourceTool {
    fn name(&self) -> &str {
        "ReadMcpResource"
    }

    fn description(&self) -> &str {
        "Read a specific resource from an MCP server by URI. Use ListMcpResources first to discover available resources."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "The MCP server name"
                },
                "uri": {
                    "type": "string",
                    "description": "The resource URI to read"
                }
            },
            "required": ["server", "uri"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let server_name = input
            .get("server")
            .and_then(|s| s.as_str())
            .unwrap_or_default();
        let uri = input
            .get("uri")
            .and_then(|s| s.as_str())
            .unwrap_or_default();

        if server_name.is_empty() {
            return ToolResult::error(serde_json::json!({
                "status": "error",
                "message": "server parameter is required",
                "data": null
            }).to_string());
        }
        if uri.is_empty() {
            return ToolResult::error(serde_json::json!({
                "status": "error",
                "message": "uri parameter is required",
                "data": null
            }).to_string());
        }

        let clients = self.clients.lock().await;

        // Find client by server name — use lock().await instead of blocking_lock()
        // to avoid deadlocking the tokio runtime.
        let mut client_arc: Option<Arc<Mutex<McpClient>>> = None;
        for c in clients.iter() {
            let client = c.lock().await;
            if client.name() == server_name {
                drop(client);
                client_arc = Some(c.clone());
                break;
            }
        }

        if client_arc.is_none() {
            let mut available_servers = Vec::new();
            for c in clients.iter() {
                let client = c.lock().await;
                available_servers.push(client.name().to_string());
            }
            return ToolResult::error(serde_json::json!({
                "status": "error",
                "message": format!("Server '{}' not found. Available servers: {}", server_name, available_servers.join(", ")),
                "data": null
            }).to_string());
        }

        let client = client_arc.as_ref().unwrap().lock().await;

        // Send resources/read request
        let result = client
            .send_request(
                "resources/read",
                Some(serde_json::json!({
                    "uri": uri
                })),
            )
            .await;

        match result {
            Ok(response) => {
                let contents = response
                    .get("contents")
                    .and_then(|v| v.as_array())
                    .ok_or("invalid resources/read response");

                match contents {
                    Ok(content_array) => {
                        let mut output_contents: Vec<ResourceContent> = Vec::new();

                        for content in content_array {
                            let content_uri = content
                                .get("uri")
                                .and_then(|u| u.as_str())
                                .unwrap_or(uri)
                                .to_string();
                            let mime_type = content
                                .get("mimeType")
                                .and_then(|m| m.as_str())
                                .map(|s| s.to_string());

                            // Handle text content
                            if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
                                output_contents.push(ResourceContent {
                                    uri: content_uri,
                                    mime_type,
                                    text: Some(text.to_string()),
                                    blob_saved_to: None,
                                });
                            }
                            // Handle blob content (base64 encoded binary)
                            else if let Some(blob) = content.get("blob").and_then(|b| b.as_str())
                            {
                                // For now, we just note that binary content exists
                                // In a full implementation, we would decode and save to a file
                                let uri_ref = content_uri.clone();
                                output_contents.push(ResourceContent {
                                    uri: content_uri,
                                    mime_type,
                                    text: Some(format!(
                                        "[Binary content: {} bytes base64-encoded. URI: {}]",
                                        blob.len(),
                                        uri_ref
                                    )),
                                    blob_saved_to: None,
                                });
                            }
                            // Unknown content type
                            else {
                                output_contents.push(ResourceContent {
                                    uri: content_uri,
                                    mime_type,
                                    text: Some(content.to_string()),
                                    blob_saved_to: None,
                                });
                            }
                        }

                        let output = ReadMcpResourceOutput {
                            contents: output_contents,
                        };
                        let data = serde_json::to_value(&output).unwrap_or_default();
                        ToolResult::success(serde_json::json!({
                            "status": "success",
                            "message": format!("Read {} resource content(s)", output.contents.len()),
                            "data": data
                        }).to_string())
                    }
                    Err(e) => ToolResult::error(serde_json::json!({
                        "status": "error",
                        "message": format!("Invalid response: {}", e),
                        "data": null
                    }).to_string()),
                }
            }
            Err(e) => ToolResult::error(serde_json::json!({
                "status": "error",
                "message": format!("Failed to read resource: {}", e),
                "data": null
            }).to_string()),
        }
    }
}
