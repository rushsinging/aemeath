use super::config::{ConnectionState, McpManagerConfig, McpServerConnection};
use super::diff::qualified_tool_name;
use super::wrapper::McpToolWrapper;
use crate::mcp::{McpClient, McpServerConfig, McpToolDef};
use crate::tool::ToolRegistry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct McpConnectionManager {
    /// Configuration
    config: McpManagerConfig,
    /// Server connections
    connections: Arc<Mutex<HashMap<String, McpServerConnection>>>,
    /// All discovered tools (server_name -> tools)
    discovered_tools: Arc<Mutex<HashMap<String, Vec<McpToolDef>>>>,
}

impl McpConnectionManager {
    /// Create a new connection manager
    pub fn new(config: McpManagerConfig) -> Self {
        Self {
            config,
            connections: Arc::new(Mutex::new(HashMap::new())),
            discovered_tools: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create with default configuration
    pub fn with_servers(servers: HashMap<String, McpServerConfig>) -> Self {
        let config = McpManagerConfig {
            servers,
            ..Default::default()
        };
        Self::new(config)
    }

    /// Load configuration from JSON file
    pub fn load_from_file(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let config: McpManagerConfig =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

        Ok(Self::new(config))
    }

    /// Initialize all configured servers
    pub async fn initialize(&self) -> Result<(), String> {
        let mut connections = self.connections.lock().await;

        for (name, config) in &self.config.servers {
            let connection = McpServerConnection::initializing(name.clone(), config.clone());
            connections.insert(name.clone(), connection);
        }

        Ok(())
    }

    /// Connect to a specific server
    pub async fn connect_server(&self, name: &str) -> Result<McpServerConnection, String> {
        let connection = {
            let connections = self.connections.lock().await;
            connections
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Server '{}' not configured", name))?
        };

        // Attempt connection
        let client = McpClient::connect(name, &connection.config).await;

        let mut connections = self.connections.lock().await;

        match client {
            Ok(client) => {
                // Discover tools
                let tools = client.list_tools().await.unwrap_or_default();

                // Check for resource support
                let supports_resources = self.check_resource_support(&client).await;

                let client_arc = Arc::new(Mutex::new(client));

                let updated = McpServerConnection {
                    name: name.to_string(),
                    config: connection.config.clone(),
                    state: ConnectionState::Connected,
                    client: Some(client_arc.clone()),
                    tools: tools.clone(),
                    error: None,
                    supports_resources,
                };

                connections.insert(name.to_string(), updated.clone());

                // Store discovered tools
                let mut discovered = self.discovered_tools.lock().await;
                discovered.insert(name.to_string(), tools);

                Ok(updated)
            }
            Err(e) => {
                let updated = McpServerConnection::failed(
                    name.to_string(),
                    connection.config.clone(),
                    e.clone(),
                );

                connections.insert(name.to_string(), updated.clone());
                Err(e)
            }
        }
    }

    /// Connect all configured servers
    pub async fn connect_all(&self) -> HashMap<String, Result<McpServerConnection, String>> {
        let mut results = HashMap::new();

        for name in self.config.servers.keys() {
            let result = self.connect_server(name).await;
            results.insert(name.clone(), result);
        }

        results
    }

    /// Disconnect a specific server
    pub async fn disconnect_server(&self, name: &str) -> Result<(), String> {
        let mut connections = self.connections.lock().await;

        if let Some(connection) = connections.get_mut(name) {
            if let Some(client) = &connection.client {
                let mut client = client.lock().await;
                client.shutdown().await;
            }

            connection.state = ConnectionState::Disabled;
            connection.client = None;
            connection.tools.clear();
        }

        Ok(())
    }

    /// Reconnect a server (useful after failure)
    pub async fn reconnect_server(&self, name: &str) -> Result<McpServerConnection, String> {
        let mut connections = self.connections.lock().await;

        if let Some(connection) = connections.get_mut(name) {
            connection.state = ConnectionState::Reconnecting;
            connection.error = None;
        }

        drop(connections);

        self.connect_server(name).await
    }

    /// Mark a server as reconnecting and clear any stored error.
    pub async fn mark_reconnecting(&self, name: &str) -> Result<(), String> {
        let mut connections = self.connections.lock().await;
        let connection = connections
            .get_mut(name)
            .ok_or_else(|| format!("Server '{}' not found", name))?;

        connection.state = ConnectionState::Reconnecting;
        connection.error = None;

        Ok(())
    }

    /// Mark a server as failed, store the error, and clear any active client.
    pub(crate) async fn set_failed(&self, name: &str, error: String) -> Result<(), String> {
        let mut connections = self.connections.lock().await;
        let connection = connections
            .get_mut(name)
            .ok_or_else(|| format!("Server '{}' not found", name))?;

        connection.state = ConnectionState::Failed;
        connection.error = Some(error);
        connection.client = None;

        Ok(())
    }

    /// Refresh the cached tool snapshot for a server.
    pub async fn refresh_tool_snapshot(
        &self,
        name: &str,
        tools: Vec<McpToolDef>,
    ) -> Result<(), String> {
        {
            let mut connections = self.connections.lock().await;
            let connection = connections
                .get_mut(name)
                .ok_or_else(|| format!("Server '{}' not found", name))?;
            connection.tools = tools.clone();
        }

        let mut discovered = self.discovered_tools.lock().await;
        discovered.insert(name.to_string(), tools);

        Ok(())
    }

    /// Run one health-check pass across connected servers.
    pub async fn health_check_once(&self) {
        let connected_servers = self.get_connected_servers().await;

        for server in connected_servers {
            let Some(client) = server.client else {
                continue;
            };

            let ping_result = {
                let client = client.lock().await;
                client.ping().await
            };

            if let Err(error) = ping_result {
                log::warn!(
                    "MCP server '{}' health check failed: {}",
                    server.name,
                    error
                );

                if self.config.auto_reconnect {
                    if let Err(reconnect_error) = self.reconnect_server(&server.name).await {
                        log::warn!(
                            "Failed to reconnect MCP server '{}': {}",
                            server.name,
                            reconnect_error
                        );

                        if let Err(set_failed_error) =
                            self.set_failed(&server.name, reconnect_error).await
                        {
                            log::warn!(
                                "Failed to mark MCP server '{}' as failed: {}",
                                server.name,
                                set_failed_error
                            );
                        }
                    }
                } else if let Err(mark_error) = self.mark_reconnecting(&server.name).await {
                    log::warn!(
                        "Failed to mark MCP server '{}' as reconnecting: {}",
                        server.name,
                        mark_error
                    );
                }
            }
        }
    }

    /// Toggle server enabled/disabled
    pub async fn toggle_server(&self, name: &str) -> Result<(), String> {
        // Read state while holding the lock, then release before calling
        // other methods that need the same lock.
        let should_enable = {
            let connections = self.connections.lock().await;
            let connection = connections.get(name).cloned();
            match connection {
                None => return Err(format!("Server '{}' not found", name)),
                Some(c) => c.state == ConnectionState::Disabled,
            }
        }; // lock released here

        if should_enable {
            self.reconnect_server(name).await?;
        } else {
            self.disconnect_server(name).await?;
        }

        Ok(())
    }

    /// Get a server connection
    pub async fn get_server(&self, name: &str) -> Option<McpServerConnection> {
        let connections = self.connections.lock().await;
        connections.get(name).cloned()
    }

    /// Get all server connections
    pub async fn get_all_servers(&self) -> Vec<McpServerConnection> {
        let connections = self.connections.lock().await;
        connections.values().cloned().collect()
    }

    /// Get connected servers
    pub async fn get_connected_servers(&self) -> Vec<McpServerConnection> {
        let connections = self.connections.lock().await;
        connections
            .values()
            .filter(|c| c.state == ConnectionState::Connected)
            .cloned()
            .collect()
    }

    /// Get all discovered tools
    pub async fn get_all_tools(&self) -> Vec<(String, McpToolDef)> {
        let discovered = self.discovered_tools.lock().await;
        discovered
            .iter()
            .flat_map(|(server, tools)| tools.iter().map(|t| (server.clone(), t.clone())))
            .collect()
    }

    /// Register MCP tools into a tool registry
    pub async fn register_tools(&self, registry: &mut ToolRegistry) {
        let connections = self.connections.lock().await;

        for connection in connections.values() {
            if connection.state != ConnectionState::Connected {
                continue;
            }

            if let Some(client_arc) = &connection.client {
                for tool_def in &connection.tools {
                    // Create qualified name: mcp__server__tool
                    let qualified_name = qualified_tool_name(&connection.name, &tool_def.name);

                    let mcp_tool = McpToolWrapper {
                        tool_name: tool_def.name.clone(),
                        qualified_name: qualified_name.clone(),
                        description: tool_def.description.clone(),
                        schema: tool_def.input_schema.clone(),
                        client: client_arc.clone(),
                    };

                    registry.register(Box::new(mcp_tool));
                }
            }
        }
    }

    /// Check if server supports resources
    async fn check_resource_support(&self, client: &McpClient) -> bool {
        // Try to list resources, if successful then supports resources
        client.send_request("resources/list", None).await.is_ok()
    }

    /// Shutdown all connections
    pub async fn shutdown(&self) {
        let mut connections = self.connections.lock().await;

        for connection in connections.values_mut() {
            if let Some(client) = &connection.client {
                let mut client = client.lock().await;
                client.shutdown().await;
            }
            connection.state = ConnectionState::Disabled;
            connection.client = None;
        }
    }
}
