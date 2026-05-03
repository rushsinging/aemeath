use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// MCP server configuration (stdio transport)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// An MCP tool definition received from a server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

/// JSON-RPC request
#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

/// JSON-RPC response
#[derive(Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

/// MCP stdio client — communicates with an MCP server via stdin/stdout
pub struct McpClient {
    name: String,
    child: Child,
    stdin: Mutex<tokio::process::ChildStdin>,
    stdout: Mutex<BufReader<tokio::process::ChildStdout>>,
    next_id: Mutex<u64>,
}

/// Environment variable keys that are too dangerous to allow MCP servers to override.
const BLOCKED_ENV_KEYS: &[&str] = &[
    "PATH",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "HOME",
    "USER",
    "SHELL",
    "IFS",
    "CDPATH",
    "ENV",
    "BASH_ENV",
    "TERMINFO",
    "TERMINFO_DIRS",
    "LOCPATH",
    "NLSPATH",
];

/// Validate that the MCP server command is safe to execute.
///
/// Rejects:
/// - Relative paths (must be absolute)
/// - Shell metacharacters (`|`, `&`, `;`, `$`, backticks, `>`, `<`, `(`, `)`)
/// - Known shell names (sh, bash, zsh, fish, etc.)
fn validate_command(command: &str) -> Result<(), String> {
    if command.contains('|')
        || command.contains('&')
        || command.contains(';')
        || command.contains('$')
        || command.contains('`')
        || command.contains('>')
        || command.contains('<')
        || command.contains('(')
        || command.contains(')')
    {
        return Err(format!(
            "MCP command '{}' contains shell metacharacters — rejected for security",
            command
        ));
    }

    if !command.starts_with('/') {
        return Err(format!(
            "MCP command '{}' must be an absolute path — rejected for security",
            command
        ));
    }

    // Block obvious shell invocations
    let basename = command.rsplit('/').next().unwrap_or(command);
    let blocked_commands = [
        "sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "python", "python3", "node",
        "ruby", "perl", "lua",
    ];
    if blocked_commands.contains(&basename) {
        return Err(format!(
            "MCP command '{}' is a shell/interpreter — use the actual executable path instead",
            command
        ));
    }

    Ok(())
}

/// Filter out dangerous environment variables from the MCP server config.
fn filter_env(
    env: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    env.iter()
        .filter(|(k, _)| {
            let upper = k.to_uppercase();
            !BLOCKED_ENV_KEYS.contains(&upper.as_str())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

impl McpClient {
    /// Connect to an MCP server via stdio
    pub async fn connect(name: &str, config: &McpServerConfig) -> Result<Self, String> {
        // Validate command safety
        validate_command(&config.command)?;

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()); // Capture stderr for debugging instead of discarding

        // Filter dangerous environment variables
        let safe_env = filter_env(&config.env);
        for (k, v) in &safe_env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to start MCP server '{}': {e}", config.command))?;

        let stdin = child.stdin.take().ok_or("failed to get stdin")?;
        let stdout = child.stdout.take().ok_or("failed to get stdout")?;
        let stderr = child.stderr.take().ok_or("failed to get stderr")?;

        // Spawn a background task to log stderr output
        let server_name = name.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                log::warn!("[MCP:{}:stderr] {}", server_name, line);
            }
        });

        let client = Self {
            name: name.to_string(),
            child,
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            next_id: Mutex::new(1),
        };

        // Initialize the connection
        client
            .send_request(
                "initialize",
                Some(serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "aemeath",
                        "version": "0.1.0"
                    }
                })),
            )
            .await?;

        // Send initialized notification
        client
            .send_notification("notifications/initialized", None)
            .await?;

        Ok(client)
    }

    pub async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        let mut id = self.next_id.lock().await;
        let req_id = *id;
        *id += 1;
        drop(id);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: req_id,
            method: method.to_string(),
            params,
        };

        let mut line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        line.push('\n');

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write error: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("flush error: {e}"))?;
        drop(stdin);

        // Read response
        let mut stdout = self.stdout.lock().await;
        let mut response_line = String::new();
        stdout
            .read_line(&mut response_line)
            .await
            .map_err(|e| format!("read error: {e}"))?;
        drop(stdout);

        let response: JsonRpcResponse = serde_json::from_str(&response_line)
            .map_err(|e| format!("invalid JSON-RPC response: {e}"))?;

        if let Some(error) = response.error {
            return Err(format!("MCP error: {}", error.message));
        }

        response.result.ok_or_else(|| "empty result".to_string())
    }

    async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(Value::Object(serde_json::Map::new()))
        });

        let mut line = serde_json::to_string(&notification).map_err(|e| e.to_string())?;
        line.push('\n');

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write error: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("flush error: {e}"))?;

        Ok(())
    }

    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, String> {
        let result = self.send_request("tools/list", None).await?;
        let tools = result
            .get("tools")
            .and_then(|v| v.as_array())
            .ok_or("invalid tools response")?;

        let mut defs = Vec::new();
        for tool in tools {
            if let Ok(def) = serde_json::from_value::<McpToolDef>(tool.clone()) {
                defs.push(def);
            }
        }
        Ok(defs)
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String, String> {
        let result = self
            .send_request(
                "tools/call",
                Some(serde_json::json!({
                    "name": name,
                    "arguments": arguments
                })),
            )
            .await?;

        // Extract text content from result
        if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                .collect();
            Ok(texts.join("\n"))
        } else {
            Ok(result.to_string())
        }
    }

    /// Get the server name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Shutdown the MCP server
    pub async fn shutdown(&mut self) {
        let _ = self.child.kill().await;
    }
}
