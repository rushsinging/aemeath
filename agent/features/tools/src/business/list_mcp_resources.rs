//! List resources from connected MCP servers

use crate::api::{Tool, ToolExecutionContext, ToolResult};
use crate::business::mcp::McpClient;
use crate::LOG_TARGET;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// List MCP resources tool
pub struct ListMcpResourcesTool {
    pub clients: Arc<Mutex<Vec<Arc<Mutex<McpClient>>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    /// Resource URI
    pub uri: String,
    /// Resource name
    pub name: String,
    /// MIME type of the resource
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Resource description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Server that provides this resource
    pub server: String,
}

#[async_trait]
impl Tool for ListMcpResourcesTool {
    type Result = ToolResult;
    fn name(&self) -> &str {
        "ListMcpResources"
    }

    fn description(&self) -> &str {
        "List resources available from connected MCP servers. Resources can be files, data, or other content that MCP servers provide access to."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Optional server name to filter resources by"
                }
            }
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let server_filter = input.get("server").and_then(|s| s.as_str());

        let clients = self.clients.lock().await;

        // Collect client info first (name and arc) without blocking_lock
        let mut client_info: Vec<(String, Arc<Mutex<McpClient>>)> = Vec::new();
        for c in clients.iter() {
            let client = c.lock().await;
            client_info.push((client.name().to_string(), c.clone()));
        }

        // Filter by server name if provided
        let clients_to_process: Vec<(String, Arc<Mutex<McpClient>>)> =
            if let Some(server_name) = server_filter {
                client_info
                    .into_iter()
                    .filter(|(name, _)| name == server_name)
                    .collect()
            } else {
                client_info
            };

        if let Some(filter) = server_filter {
            if clients_to_process.is_empty() {
                let mut available_servers = Vec::new();
                for c in clients.iter() {
                    let client = c.lock().await;
                    available_servers.push(client.name().to_string());
                }
                return ToolResult::error(serde_json::json!({
                    "status": "error",
                    "message": format!("Server '{}' not found. Available servers: {}", filter, available_servers.join(", ")),
                    "data": null
                }).to_string());
            }
        }

        let mut resources: Vec<McpResource> = Vec::new();

        for (server_name, client_arc) in clients_to_process {
            let client = client_arc.lock().await;
            match Self::list_resources(&client).await {
                Ok(res) => {
                    for r in res {
                        resources.push(McpResource {
                            uri: r.uri,
                            name: r.name,
                            mime_type: r.mime_type,
                            description: r.description,
                            server: server_name.clone(),
                        });
                    }
                }
                Err(e) => {
                    // Log error but continue with other servers
                    log::warn!(target: LOG_TARGET, "Failed to list resources from {}: {}", server_name, e);
                }
            }
        }

        if resources.is_empty() {
            ToolResult::success(serde_json::json!({
                "status": "success",
                "message": "No resources found. MCP servers may still provide tools even if they have no resources.",
                "data": null
            }).to_string())
        } else {
            let data = serde_json::to_value(&resources).unwrap_or_default();
            ToolResult::success(
                serde_json::json!({
                    "status": "success",
                    "message": format!("Found {} resource(s)", resources.len()),
                    "data": data
                })
                .to_string(),
            )
        }
    }
}

impl ListMcpResourcesTool {
    async fn list_resources(client: &McpClient) -> Result<Vec<McpResourceRaw>, String> {
        let result = client.send_request("resources/list", None).await?;

        let resources = result
            .get("resources")
            .and_then(|v| v.as_array())
            .ok_or("invalid resources response")?;

        let mut defs = Vec::new();
        for res in resources {
            if let Ok(def) = serde_json::from_value::<McpResourceRaw>(res.clone()) {
                defs.push(def);
            }
        }
        Ok(defs)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct McpResourceRaw {
    uri: String,
    name: String,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    description: Option<String>,
}
