use aemeath_core::tool::ToolRegistry;
use std::path::PathBuf;

pub async fn load_mcp_tools(
    registry: &mut ToolRegistry,
    cwd: &PathBuf,
) -> Vec<std::sync::Arc<tokio::sync::Mutex<aemeath_core::mcp::McpClient>>> {
    use aemeath_core::mcp::{McpClient, McpServerConfig};
    use aemeath_tools::mcp_tool::McpTool;

    let mut clients = Vec::new();

    // Look for MCP config in .mcp.json or ~/.aemeath/mcp.json
    let config_paths = [
        cwd.join(".mcp.json"),
        dirs::home_dir()
            .map(|h| h.join(".aemeath").join("mcp.json"))
            .unwrap_or_default(),
    ];

    for config_path in &config_paths {
        if !config_path.exists() {
            continue;
        }

        // Warn if loading from a project-level config (may contain untrusted commands)
        let is_project_level = config_path == &cwd.join(".mcp.json");
        if is_project_level {
            eprintln!(
                "⚠️  SECURITY: Loading MCP servers from project config {}.\n\
                 These servers can execute arbitrary commands on your system.\n\
                 Review the commands before proceeding. Use --no-tui and Ctrl+C to abort if needed.",
                config_path.display()
            );
            log::warn!(
                "Loading MCP servers from project-level config {} — commands may be untrusted.",
                config_path.display()
            );
        }

        let content = match tokio::fs::read_to_string(config_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("invalid MCP config {}: {e}", config_path.display());
                continue;
            }
        };

        // Expect format: { "mcpServers": { "name": { "command": "...", "args": [...] } } }
        let servers = match config.get("mcpServers").and_then(|v| v.as_object()) {
            Some(s) => s,
            None => continue,
        };

        for (name, server_config) in servers {
            let mcp_config: McpServerConfig = match serde_json::from_value(server_config.clone()) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("invalid MCP server config '{}': {e}", name);
                    continue;
                }
            };

            log::info!("[MCP] connecting to {}...", name);
            match McpClient::connect(name, &mcp_config).await {
                Ok(client) => {
                    let client = std::sync::Arc::new(tokio::sync::Mutex::new(client));

                    // Fetch and register tools
                    match client.lock().await.list_tools().await {
                        Ok(tools) => {
                            log::info!("[MCP] {} registered {} tools", name, tools.len());
                            for tool_def in tools {
                                let qualified = format!("mcp__{}_{}", name, tool_def.name);
                                registry.register(Box::new(McpTool {
                                    tool_name: tool_def.name,
                                    qualified_name: qualified,
                                    tool_description: tool_def.description,
                                    schema: tool_def.input_schema,
                                    client: client.clone(),
                                }));
                            }
                        }
                        Err(e) => log::warn!("[MCP] {} failed to list tools: {e}", name),
                    }

                    clients.push(client);
                }
                Err(e) => log::warn!("[MCP] failed to connect to {}: {e}", name),
            }
        }
    }

    clients
}
