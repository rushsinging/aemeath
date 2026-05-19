//! SSE transport for MCP.
//!
//! The MCP SSE protocol works as follows:
//! 1. Client connects to the SSE URL via GET
//! 2. Server sends an `endpoint` event containing the JSON-RPC POST URL
//! 3. Client sends JSON-RPC requests via POST to that URL
//! 4. Server delivers JSON-RPC responses as SSE `message` events

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Notify};

/// Parsed SSE event
#[derive(Debug, Clone)]
pub(crate) struct SseEvent {
    pub event_type: String,
    pub data: String,
}

/// Parse a single SSE event block from raw text.
///
/// SSE format:
/// ```text
/// event: <type>
/// data: <payload>
/// ```
/// Lines are separated by `\n\n` (double newline).
pub fn parse_sse_events(raw: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    for block in raw.split("\n\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        let mut event_type = String::from("message");
        let mut data = String::new();
        for line in block.lines() {
            if let Some(val) = line.strip_prefix("event:") {
                event_type = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("data:") {
                if data.is_empty() {
                    data = val.trim().to_string();
                } else {
                    data.push('\n');
                    data.push_str(val.trim());
                }
            }
        }
        if !data.is_empty() {
            events.push(SseEvent { event_type, data });
        }
    }
    events
}

/// Extract the JSON-RPC POST endpoint URL from the initial SSE `endpoint` event.
///
/// The server sends a relative or absolute path; it MUST be resolved against
/// the original SSE URL's base.
pub fn resolve_endpoint_url(base_url: &str, endpoint_path: &str) -> Result<String, String> {
    let base = url::Url::parse(base_url).map_err(|e| format!("invalid base url: {e}"))?;
    let resolved = base
        .join(endpoint_path)
        .map_err(|e| format!("failed to resolve endpoint url: {e}"))?;
    Ok(resolved.to_string())
}

/// Default timeout for SSE endpoint handshake (seconds).
const SSE_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Default timeout for individual JSON-RPC requests (seconds).
const SSE_REQUEST_TIMEOUT_SECS: u64 = 30;

/// Build a reqwest client with optional custom headers and sane timeouts.
pub fn build_http_client(headers: &HashMap<String, String>) -> Result<Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(SSE_CONNECT_TIMEOUT_SECS))
        .timeout(std::time::Duration::from_secs(SSE_REQUEST_TIMEOUT_SECS));
    // Attach default headers if provided
    if !headers.is_empty() {
        let mut header_map = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                .map_err(|e| format!("invalid header name '{key}': {e}"))?;
            let val = reqwest::header::HeaderValue::from_str(value)
                .map_err(|e| format!("invalid header value for '{key}': {e}"))?;
            header_map.insert(name, val);
        }
        builder = builder.default_headers(header_map);
    }
    builder.build().map_err(|e| format!("failed to build HTTP client: {e}"))
}

/// SSE transport handle returned after a successful connection.
///
/// The SSE connection runs in a background task that pushes parsed events
/// into an `mpsc` channel.  Callers await `next_response_for(id)` to
/// receive a specific JSON-RPC response.
pub struct SseTransport {
    /// HTTP client (shares connection pool and default headers)
    http_client: Client,
    /// JSON-RPC POST endpoint resolved from the `endpoint` SSE event
    endpoint_url: String,
    /// Channel sender — the background SSE reader pushes events here
    _event_tx: mpsc::Sender<SseEvent>,
    /// Channel receiver — consumers read events from here
    event_rx: Arc<Mutex<mpsc::Receiver<SseEvent>>>,
    /// Notifier for when a new event arrives
    notify: Arc<Notify>,
}

impl SseTransport {
    /// Connect to an SSE MCP server.
    ///
    /// 1. GET the SSE URL, parse the stream until an `endpoint` event arrives.
    /// 2. Start a background task that continues reading events.
    /// 3. Return an `SseTransport` ready for JSON-RPC communication.
    pub async fn connect(
        sse_url: &str,
        headers: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let http_client = build_http_client(headers)?;

        // Phase 1: connect and read until we get the endpoint event
        let response = http_client
            .get(sse_url)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| format!("SSE connect failed: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("SSE connect returned {status}: {body}"));
        }

        let (event_tx, event_rx) = mpsc::channel::<SseEvent>(256);
        let notify = Arc::new(Notify::new());

        // We need to read the first chunk(s) to find the endpoint event,
        // then hand off the stream to a background task.
        let (endpoint_url, stream) = {
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut endpoint_url: Option<String> = None;

            let deadline =
                tokio::time::Instant::now() + std::time::Duration::from_secs(SSE_CONNECT_TIMEOUT_SECS);

            // Read chunks until we get the endpoint event (with timeout)
            loop {
                let chunk_result = tokio::time::timeout_at(
                    deadline,
                    stream.next(),
                )
                .await;

                match chunk_result {
                    Ok(Some(Ok(chunk))) => {
                        buffer.push_str(&String::from_utf8_lossy(&chunk));

                        for event in parse_sse_events(&buffer) {
                            if event.event_type == "endpoint" {
                                endpoint_url = Some(resolve_endpoint_url(sse_url, &event.data)?);
                            } else {
                                let _ = event_tx.send(event).await;
                            }
                        }
                        if let Some(pos) = buffer.rfind("\n\n") {
                            buffer = buffer[pos + 2..].to_string();
                        }

                        if endpoint_url.is_some() {
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => {
                        return Err(format!("SSE read error: {e}"));
                    }
                    Ok(None) => {
                        return Err("SSE stream ended before sending 'endpoint' event".to_string());
                    }
                    Err(_) => {
                        return Err(format!(
                            "SSE endpoint handshake timed out after {}s",
                            SSE_CONNECT_TIMEOUT_SECS
                        ));
                    }
                }
            }

            let url = endpoint_url
                .ok_or_else(|| "SSE server did not send an 'endpoint' event".to_string())?;
            (url, stream)
        };

        // Phase 2: spawn background task to continue reading SSE events
        let notify_clone = notify.clone();
        let tx_clone = event_tx.clone();
        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut stream = stream;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        buffer.push_str(&String::from_utf8_lossy(&chunk));
                        for event in parse_sse_events(&buffer) {
                            let _ = tx_clone.send(event).await;
                        }
                        if let Some(pos) = buffer.rfind("\n\n") {
                            buffer = buffer[pos + 2..].to_string();
                        }
                    }
                    Err(e) => {
                        log::warn!("[MCP:SSE] stream error: {e}");
                        break;
                    }
                }
                notify_clone.notify_waiters();
            }
            log::info!("[MCP:SSE] background reader finished");
            notify_clone.notify_waiters();
        });

        Ok(Self {
            http_client,
            endpoint_url,
            _event_tx: event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            notify,
        })
    }

    /// Send a JSON-RPC request and wait for the matching response.
    ///
    /// POSTs the request body to the endpoint URL, then waits for an SSE
    /// `message` event whose JSON-RPC `id` matches.
    pub async fn send_request(&self, request_body: &Value) -> Result<Value, String> {
        let body = serde_json::to_string(request_body)
            .map_err(|e| format!("serialize request: {e}"))?;

        let req_id = request_body
            .get("id")
            .and_then(|v| v.as_u64());

        let resp = self
            .http_client
            .post(&self.endpoint_url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| format!("POST to MCP endpoint failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("MCP endpoint returned {status}: {text}"));
        }

        // For SSE transport, the response comes via the SSE event stream,
        // not from the POST response itself. The POST response is typically
        // 202 Accepted or 200 OK with empty body.
        // Wait for the response on the SSE channel.
        if let Some(id) = req_id {
            self.wait_for_response(id).await
        } else {
            // Notification — no response expected
            Ok(Value::Null)
        }
    }

    /// Wait for a JSON-RPC response with the given `id` from the SSE stream.
    async fn wait_for_response(&self, id: u64) -> Result<Value, String> {
        let mut rx = self.event_rx.lock().await;

        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(SSE_REQUEST_TIMEOUT_SECS);

        loop {
            // Check existing buffered events first
            while let Some(event) = rx.try_recv().ok() {
                if let Some(response) = try_extract_response(&event.data, id)? {
                    return Ok(response);
                }
            }

            // Wait for new events with timeout
            let notified = tokio::time::timeout_at(deadline, self.notify.notified()).await;
            if notified.is_err() {
                return Err(format!(
                    "MCP SSE response timed out after {}s (request id={})",
                    SSE_REQUEST_TIMEOUT_SECS, id
                ));
            }

            // Drain newly arrived events
            while let Some(event) = rx.try_recv().ok() {
                if let Some(response) = try_extract_response(&event.data, id)? {
                    return Ok(response);
                }
            }
        }
    }
}

/// Try to extract a JSON-RPC response matching `id` from SSE event data.
fn try_extract_response(data: &str, expected_id: u64) -> Result<Option<Value>, String> {
    let value: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Ok(None), // Not JSON, skip
    };

    // Check if this is a response with matching id
    let resp_id = value.get("id").and_then(|v| v.as_u64());
    if resp_id != Some(expected_id) {
        return Ok(None);
    }

    // Check for JSON-RPC error
    if let Some(error) = value.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("MCP error: {msg}"));
    }

    // Return the result
    if let Some(result) = value.get("result") {
        Ok(Some(result.clone()))
    } else {
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_events_single_message() {
        let raw = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n";
        let events = parse_sse_events(raw);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "message");
        assert!(events[0].data.contains("\"id\":1"));
    }

    #[test]
    fn test_parse_sse_events_endpoint() {
        let raw = "event: endpoint\ndata: /mcp/messages\n\n";
        let events = parse_sse_events(raw);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "endpoint");
        assert_eq!(events[0].data, "/mcp/messages");
    }

    #[test]
    fn test_parse_sse_events_multiple() {
        let raw = "event: endpoint\ndata: /rpc\n\nevent: message\ndata: hello\n\n";
        let events = parse_sse_events(raw);

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "endpoint");
        assert_eq!(events[1].event_type, "message");
        assert_eq!(events[1].data, "hello");
    }

    #[test]
    fn test_parse_sse_events_ignores_empty() {
        let raw = "\n\n\n\n";
        let events = parse_sse_events(raw);

        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_sse_events_default_type_is_message() {
        let raw = "data: just_data\n\n";
        let events = parse_sse_events(raw);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "message");
        assert_eq!(events[0].data, "just_data");
    }

    #[test]
    fn test_parse_sse_events_multiline_data() {
        let raw = "data: line1\ndata: line2\n\n";
        let events = parse_sse_events(raw);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2");
    }

    #[test]
    fn test_resolve_endpoint_url_relative() {
        let result = resolve_endpoint_url(
            "https://api.example.com/sse?token=abc",
            "/mcp/messages",
        )
        .unwrap();

        assert_eq!(result, "https://api.example.com/mcp/messages");
    }

    #[test]
    fn test_resolve_endpoint_url_absolute() {
        let result = resolve_endpoint_url(
            "https://api.example.com/sse",
            "https://other.example.com/rpc",
        )
        .unwrap();

        assert_eq!(result, "https://other.example.com/rpc");
    }

    #[test]
    fn test_resolve_endpoint_url_invalid_base() {
        let err = resolve_endpoint_url("not-a-url", "/path").unwrap_err();
        assert!(err.contains("invalid base url"));
    }

    #[test]
    fn test_try_extract_response_matching_id() {
        let data = r#"{"jsonrpc":"2.0","id":42,"result":{"tools":[]}}"#;
        let result = try_extract_response(data, 42).unwrap();

        assert!(result.is_some());
        let val = result.unwrap();
        assert!(val.get("tools").is_some());
    }

    #[test]
    fn test_try_extract_response_non_matching_id() {
        let data = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        let result = try_extract_response(data, 99).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_try_extract_response_error() {
        let data = r#"{"jsonrpc":"2.0","id":5,"error":{"code":-32600,"message":"invalid"}}"#;
        let err = try_extract_response(data, 5).unwrap_err();

        assert!(err.contains("invalid"));
    }

    #[test]
    fn test_try_extract_response_invalid_json() {
        let result = try_extract_response("not json", 1).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_build_http_client_no_headers() {
        let client = build_http_client(&HashMap::new());
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_http_client_with_headers() {
        let headers = HashMap::from([
            ("Authorization".to_string(), "Bearer token".to_string()),
            ("X-Custom".to_string(), "value".to_string()),
        ]);
        let client = build_http_client(&headers);
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_http_client_invalid_header_name() {
        let headers = HashMap::from([("中文键".to_string(), "value".to_string())]);
        let err = build_http_client(&headers).unwrap_err();
        assert!(err.contains("invalid header name"));
    }
}
