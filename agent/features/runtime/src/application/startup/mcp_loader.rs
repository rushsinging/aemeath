use crate::application::startup::config_paths as paths;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tools::McpConnectionManager;
use tools::McpServerConfig;
use tools::ToolRegistry;

pub fn parse_mcp_servers_config(
    config: &serde_json::Value,
) -> Result<HashMap<String, McpServerConfig>, String> {
    let Some(mcp_servers) = config.get("mcpServers") else {
        return Ok(HashMap::new());
    };

    serde_json::from_value(mcp_servers.clone())
        .map_err(|e| format!("invalid mcpServers config: {e}"))
}

pub fn merge_mcp_servers(
    base: &mut HashMap<String, McpServerConfig>,
    overlay: HashMap<String, McpServerConfig>,
) {
    base.extend(overlay);
}

async fn read_mcp_servers_config(config_path: &Path) -> Option<HashMap<String, McpServerConfig>> {
    if !config_path.exists() {
        return None;
    }

    let content = match tokio::fs::read_to_string(config_path).await {
        Ok(c) => c,
        Err(e) => {
            log::warn!(target: crate::LOG_TARGET, "failed to read MCP config {}: {e}", config_path.display());
            return None;
        }
    };

    let config: serde_json::Value = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            log::warn!(target: crate::LOG_TARGET, "invalid MCP config {}: {e}", config_path.display());
            return None;
        }
    };

    match parse_mcp_servers_config(&config) {
        Ok(servers) => Some(servers),
        Err(e) => {
            log::warn!(target: crate::LOG_TARGET, "invalid MCP config {}: {e}", config_path.display());
            None
        }
    }
}

pub async fn load_mcp_manager(cwd: &Path) -> Arc<McpConnectionManager> {
    let mut servers = HashMap::new();

    let global_config_path = paths::global_mcp_config_path();
    if let Some(global_servers) = read_mcp_servers_config(&global_config_path).await {
        merge_mcp_servers(&mut servers, global_servers);
    }

    let project_config_path = cwd.join(".mcp.json");
    if let Some(project_servers) = read_mcp_servers_config(&project_config_path).await {
        if !project_servers.is_empty() {
            log::warn!(target: crate::LOG_TARGET,
                "Loading MCP servers from project-level config {} — commands may be untrusted.",
                project_config_path.display()
            );
        }
        merge_mcp_servers(&mut servers, project_servers);
    }

    let manager = Arc::new(McpConnectionManager::with_servers(servers));
    if let Err(e) = manager.initialize().await {
        log::warn!(target: crate::LOG_TARGET, "failed to initialize MCP manager: {e}");
    }
    manager
}

/// Connect all configured MCP servers and register their tools.
///
/// Awaits completion so that tools are available before the first LLM turn.
pub async fn spawn_mcp_connect(
    registry: Arc<ToolRegistry>,
    cwd: &Path,
) -> Arc<McpConnectionManager> {
    let manager = load_mcp_manager(cwd).await;

    log::info!(target: crate::LOG_TARGET, "[MCP] connecting {} servers", manager.server_count());

    for (name, result) in manager.connect_all().await {
        match result {
            Ok(connection) => {
                log::info!(target: crate::LOG_TARGET,
                    "[MCP] {} connected with {} tools",
                    name,
                    connection.tools.len()
                );
            }
            Err(e) => {
                log::warn!(target: crate::LOG_TARGET, "[MCP] failed to connect to {}: {e}", name)
            }
        }
    }
    manager.register_tools(&registry).await;
    log::info!(target: crate::LOG_TARGET, "[MCP] all servers connected");

    manager
}

#[cfg(test)]
mod tests {
    use super::{merge_mcp_servers, parse_mcp_servers_config};
    use serde_json::json;
    use std::collections::HashMap;
    use tools::McpServerConfig;

    #[test]
    fn test_parse_mcp_servers_config_reads_mcp_servers() {
        let config = json!({
            "mcpServers": {
                "demo": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {"API_KEY": "secret"}
                }
            }
        });

        let servers = parse_mcp_servers_config(&config).expect("parse mcpServers");
        let server = servers.get("demo").expect("demo server");

        assert_eq!(server.command.as_deref(), Some("node"));
        assert_eq!(server.args, vec!["server.js"]);
        assert_eq!(
            server.env.get("API_KEY").map(String::as_str),
            Some("secret")
        );
    }

    #[test]
    fn test_parse_mcp_servers_config_empty_when_missing() {
        let config = json!({"other": {}});

        let servers = parse_mcp_servers_config(&config).expect("missing mcpServers is valid");

        assert!(servers.is_empty());
    }

    #[test]
    fn test_merge_mcp_servers_project_overrides_global() {
        let mut base = HashMap::from([(
            "demo".to_string(),
            McpServerConfig {
                command: Some("global".to_string()),
                args: vec!["global.js".to_string()],
                env: HashMap::new(),
                url: None,
                headers: HashMap::new(),
                transport: None,
            },
        )]);
        let overlay = HashMap::from([
            (
                "demo".to_string(),
                McpServerConfig {
                    command: Some("project".to_string()),
                    args: vec!["project.js".to_string()],
                    env: HashMap::new(),
                    url: None,
                    headers: HashMap::new(),
                    transport: None,
                },
            ),
            (
                "extra".to_string(),
                McpServerConfig {
                    command: Some("extra".to_string()),
                    args: Vec::new(),
                    env: HashMap::new(),
                    url: None,
                    headers: HashMap::new(),
                    transport: None,
                },
            ),
        ]);

        merge_mcp_servers(&mut base, overlay);

        assert_eq!(base.len(), 2);
        assert_eq!(base["demo"].command.as_deref(), Some("project"));
        assert_eq!(base["demo"].args, vec!["project.js"]);
        assert_eq!(base["extra"].command.as_deref(), Some("extra"));
    }
}
