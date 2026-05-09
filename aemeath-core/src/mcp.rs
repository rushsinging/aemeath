use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub transport: Option<McpTransportKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportKind {
    Stdio,
    Sse,
    StreamableHttp,
}

impl fmt::Display for McpTransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            McpTransportKind::Stdio => "stdio",
            McpTransportKind::Sse => "sse",
            McpTransportKind::StreamableHttp => "streamable_http",
        };
        f.write_str(name)
    }
}

impl McpServerConfig {
    /// Resolve the configured transport.
    ///
    /// An explicit `transport` takes precedence. Otherwise, stdio is selected
    /// when `command` is present, and streamable HTTP is selected when only
    /// `url` is present.
    pub fn transport_kind(&self) -> Result<McpTransportKind, String> {
        if let Some(kind) = self.transport {
            return Ok(kind);
        }
        if self.command.as_deref().is_some_and(|s| !s.trim().is_empty()) {
            return Ok(McpTransportKind::Stdio);
        }
        if self.url.as_deref().is_some_and(|s| !s.trim().is_empty()) {
            return Ok(McpTransportKind::StreamableHttp);
        }
        Err("MCP server config must define either command or url".to_string())
    }
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

pub const DEFAULT_MAX_TOOL_RESPONSE_BYTES: usize = 1_048_576;

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

pub fn validate_remote_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid MCP url: {e}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = parsed.host_str().unwrap_or_default();
            if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" {
                Ok(())
            } else {
                Err("remote MCP url must use https unless it points to localhost".to_string())
            }
        }
        other => Err(format!("unsupported MCP url scheme: {other}")),
    }
}

pub fn redact_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            let sensitive = lower == "authorization"
                || lower == "cookie"
                || lower == "x-api-key"
                || lower == "proxy-authorization";
            if sensitive {
                (key.clone(), "<redacted>".to_string())
            } else {
                (key.clone(), value.clone())
            }
        })
        .collect()
}

pub fn limit_tool_response(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output.to_string();
    }

    let mut truncate_at = max_bytes;
    while truncate_at > 0 && !output.is_char_boundary(truncate_at) {
        truncate_at -= 1;
    }

    let notice = format!(
        "[Output truncated: original {} bytes, limit {} bytes]",
        output.len(),
        max_bytes
    );

    if truncate_at == 0 {
        notice
    } else {
        format!("{}\n\n{}", &output[..truncate_at], notice)
    }
}

impl McpClient {
    /// Connect to an MCP server
    pub async fn connect(name: &str, config: &McpServerConfig) -> Result<Self, String> {
        let kind = config.transport_kind()?;
        match kind {
            McpTransportKind::Stdio => Self::connect_stdio(name, config).await,
            McpTransportKind::Sse | McpTransportKind::StreamableHttp => {
                Self::connect_http(name, config, kind).await
            }
        }
    }

    async fn connect_stdio(name: &str, config: &McpServerConfig) -> Result<Self, String> {
        // Validate command safety
        let command = config
            .command
            .as_deref()
            .ok_or_else(|| "stdio MCP server requires command".to_string())?;
        validate_command(command)?;

        let mut cmd = Command::new(command);
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
            .map_err(|e| format!("failed to start MCP server '{}': {e}", command))?;

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

    async fn connect_http(
        name: &str,
        config: &McpServerConfig,
        kind: McpTransportKind,
    ) -> Result<Self, String> {
        let url = config
            .url
            .as_deref()
            .ok_or_else(|| "remote MCP server requires url".to_string())?;
        validate_remote_url(url)?;
        Err(format!(
            "MCP {kind} transport for '{name}' is not yet supported"
        ))
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

    /// Send a JSON-RPC ping request to verify the MCP server is responsive.
    pub async fn ping(&self) -> Result<(), String> {
        self.send_request("ping", None).await.map(|_| ())
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

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod mcp_server_config_tests {
    use super::*;

    #[test]
    fn test_mcp_server_config_stdio_defaults_to_stdio_transport() {
        let config = McpServerConfig {
            command: Some("/usr/bin/mcp-server".to_string()),
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            transport: None,
        };

        assert_eq!(config.transport_kind().unwrap(), McpTransportKind::Stdio);
    }

    #[test]
    fn test_mcp_server_config_url_defaults_to_streamable_http() {
        let config = McpServerConfig {
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: Some("https://example.com/mcp".to_string()),
            headers: HashMap::new(),
            transport: None,
        };

        assert_eq!(
            config.transport_kind().unwrap(),
            McpTransportKind::StreamableHttp
        );
    }

    #[test]
    fn test_mcp_server_config_explicit_sse_transport() {
        let config = McpServerConfig {
            command: Some("/usr/bin/mcp-server".to_string()),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some("https://example.com/sse".to_string()),
            headers: HashMap::new(),
            transport: Some(McpTransportKind::Sse),
        };

        assert_eq!(config.transport_kind().unwrap(), McpTransportKind::Sse);
    }

    #[test]
    fn test_validate_remote_url_rejects_public_http() {
        let err = validate_remote_url("http://example.com/mcp").unwrap_err();
        assert!(err.contains("http"));
    }

    #[test]
    fn test_validate_remote_url_allows_localhost_http() {
        validate_remote_url("http://localhost:3000/mcp").unwrap();
        validate_remote_url("http://127.0.0.1:3000/mcp").unwrap();
        validate_remote_url("http://[::1]:3000/mcp").unwrap();
    }

    #[test]
    fn test_redact_headers_hides_sensitive_values() {
        let headers = HashMap::from([
            ("Authorization".to_string(), "Bearer secret".to_string()),
            ("cookie".to_string(), "session=secret".to_string()),
            ("X-Api-Key".to_string(), "secret".to_string()),
            ("Proxy-Authorization".to_string(), "secret".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ]);

        let redacted = redact_headers(&headers);

        assert_eq!(redacted.get("Authorization").unwrap(), "<redacted>");
        assert_eq!(redacted.get("cookie").unwrap(), "<redacted>");
        assert_eq!(redacted.get("X-Api-Key").unwrap(), "<redacted>");
        assert_eq!(redacted.get("Proxy-Authorization").unwrap(), "<redacted>");
        assert_eq!(redacted.get("Accept").unwrap(), "application/json");
    }

    #[test]
    fn test_limit_tool_response_keeps_small_output() {
        assert_eq!(limit_tool_response("hello", 10), "hello");
    }

    #[test]
    fn test_limit_tool_response_truncates_large_output() {
        let limited = limit_tool_response("abcdefghij", 5);

        assert!(limited.starts_with("abcde"));
        assert!(limited.contains("truncated"));
    }

    #[test]
    fn test_limit_tool_response_truncates_at_utf8_boundary() {
        let limited = limit_tool_response("你好世界", 5);

        assert!(limited.starts_with("你"));
        assert!(limited.contains("truncated"));
    }

    #[test]
    fn test_limit_tool_response_zero_limit_returns_notice_without_prefix() {
        assert_eq!(
            limit_tool_response("abc", 0),
            "[Output truncated: original 3 bytes, limit 0 bytes]"
        );
    }

    #[test]
    fn test_limit_tool_response_empty_input_with_zero_limit_returns_empty() {
        assert_eq!(limit_tool_response("", 0), "");
    }
}
