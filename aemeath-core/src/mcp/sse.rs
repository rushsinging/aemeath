//! SSE transport for MCP.
//!
//! The MCP SSE protocol works as follows:
//! 1. Client connects to the SSE URL via GET
//! 2. Server sends an `endpoint` event containing the JSON-RPC POST URL
//! 3. Client sends JSON-RPC requests via POST to that URL
//! 4. Server delivers JSON-RPC responses as SSE `message` events on the same GET stream
//!
//! Implementation: After discovering the endpoint, we keep the SSE GET stream alive.
//! Each `send_request` POSTs the request then directly reads the response from the
//! stream — no background task, no channel, no race conditions.

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Parsed SSE event
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event_type: String,
    pub data: String,
}

/// Parse SSE event blocks from raw text.
///
/// SSE format: `event: <type>\ndata: <payload>\n\n`
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

/// Resolve a relative endpoint path against the SSE base URL.
pub fn resolve_endpoint_url(base_url: &str, endpoint_path: &str) -> Result<String, String> {
    let base = url::Url::parse(base_url).map_err(|e| format!("invalid base url: {e}"))?;
    let resolved = base
        .join(endpoint_path)
        .map_err(|e| format!("failed to resolve endpoint url: {e}"))?;
    Ok(resolved.to_string())
}

/// Default timeout for SSE endpoint handshake (seconds).
const SSE_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Default timeout for individual JSON-RPC requests via SSE stream (seconds).
const SSE_REQUEST_TIMEOUT_SECS: u64 = 30;

/// Build a reqwest client with optional custom headers and sane timeouts.
pub fn build_http_client(headers: &HashMap<String, String>) -> Result<Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(SSE_CONNECT_TIMEOUT_SECS));
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
    builder
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

/// SSE transport handle.
///
/// The SSE GET stream is wrapped in `Mutex<SseReadStream>` so that
/// `send_request` can exclusively read the next response.
pub struct SseTransport {
    http_client: Client,
    endpoint_url: String,
    stream: Arc<Mutex<SseReadStream>>,
}

/// Wrapper holding the boxed SSE byte stream and a leftover buffer.
struct SseReadStream {
    buffer: String,
    stream: std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
}

impl SseReadStream {
    /// Read the next chunk from the stream and append to buffer.
    async fn read_chunk(&mut self) -> Result<bool, String> {
        match self.stream.next().await {
            Some(Ok(chunk)) => {
                let len = chunk.len();
                self.buffer.push_str(&String::from_utf8_lossy(&chunk));
                log::info!("[MCP:SSE] read_chunk: {len} bytes, buffer now {} bytes", self.buffer.len());
                if self.buffer.len() > 100 {
                    let preview: String = self.buffer.chars().take(200).collect();
                    log::info!("[MCP:SSE] buffer preview: {preview:?}");
                }
                Ok(true)
            }
            Some(Err(e)) => {
                log::warn!("[MCP:SSE] read_chunk error: {e}");
                Err(format!("SSE read error: {e}"))
            }
            None => {
                log::info!("[MCP:SSE] read_chunk: stream EOF");
                Ok(false)
            }
        }
    }

    /// Drain all complete SSE events from the buffer, returning them.
    /// Also attempts to parse events that may be missing the trailing \n\n
    /// (some servers don't send the delimiter for the last event before a pause).
    fn drain_events(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();
            if let Some(event) = parse_single_event(&block) {
                events.push(event);
            }
        }

        // Fallback: if buffer has data but no \n\n delimiter, try to parse
        // the event anyway (handles servers that omit the trailing \n\n).
        // Only attempt this if the buffer contains what looks like a complete
        // JSON-RPC response (matching braces in the data line).
        if events.is_empty() && !self.buffer.is_empty() {
            if let Some(event) = try_parse_incomplete_event(&self.buffer) {
                log::info!("[MCP:SSE] parsed incomplete event (no trailing \\n\\n)");
                self.buffer.clear();
                events.push(event);
            }
        }

        events
    }
}

/// Parse a single SSE event block (the text between \n\n delimiters).
fn parse_single_event(block: &str) -> Option<SseEvent> {
    let block = block.trim();
    if block.is_empty() {
        return None;
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
        log::info!("[MCP:SSE] drain: event={event_type} data_len={}", data.len());
        Some(SseEvent { event_type, data })
    } else {
        None
    }
}

/// Try to parse an SSE event that may be missing the trailing \n\n.
/// Checks if the data line contains valid JSON with balanced braces.
fn try_parse_incomplete_event(buffer: &str) -> Option<SseEvent> {
      // Extract the data: value
      let data_start = buffer.find("data:")?;
      let data_content = &buffer[data_start + 5..];

      log::info!("[MCP:SSE] try_parse_incomplete: data_content_len={}", data_content.len());

      // Check if the data looks like complete JSON (balanced braces)
      let mut brace_depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;
    for ch in data_content.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' | '[' => brace_depth += 1,
            '}' | ']' => brace_depth -= 1,
            _ => {}
        }
    }

    if brace_depth != 0 {
        log::info!("[MCP:SSE] try_parse_incomplete: brace_depth={brace_depth}, not balanced");
        return None; // Unbalanced braces — JSON not complete yet
    }

    // Looks complete — parse the event normally
    let event_part = &buffer[..data_start];
    let event_type = event_part
        .lines()
        .find_map(|line| line.strip_prefix("event:"))
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "message".to_string());

    let data = data_content.trim().to_string();
    log::info!("[MCP:SSE] incomplete-fallback: event={event_type} data_len={}", data.len());
    Some(SseEvent { event_type, data })
}

impl SseTransport {
    /// Connect to an SSE MCP server.
    pub async fn connect(
        sse_url: &str,
        headers: &HashMap<String, String>,
    ) -> Result<Self, String> {
        let http_client = build_http_client(headers)?;

        log::info!("[MCP:SSE] connecting to {sse_url}");

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

        let mut stream_state = SseReadStream {
            buffer: String::new(),
            stream: Box::pin(response.bytes_stream()),
        };

        // Read until we get the endpoint event
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(SSE_CONNECT_TIMEOUT_SECS);

        let mut endpoint_url: Option<String> = None;

        loop {
            // Check for complete events in buffer
            for event in stream_state.drain_events() {
                if event.event_type == "endpoint" {
                    endpoint_url = Some(resolve_endpoint_url(sse_url, &event.data)?);
                }
            }
            if endpoint_url.is_some() {
                break;
            }

            // Read more data with timeout
            let chunk_result = tokio::time::timeout_at(deadline, stream_state.read_chunk()).await;
            match chunk_result {
                Ok(Ok(true)) => {} // got data, loop to parse
                Ok(Ok(false)) => {
                    return Err(
                        "SSE stream ended before sending 'endpoint' event".to_string(),
                    );
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(format!(
                        "SSE endpoint handshake timed out after {}s",
                        SSE_CONNECT_TIMEOUT_SECS
                    ));
                }
            }
        }

        let endpoint_url = endpoint_url
            .ok_or_else(|| "SSE server did not send an 'endpoint' event".to_string())?;

        log::info!("[MCP:SSE] connected, endpoint={endpoint_url}");

        Ok(Self {
            http_client,
            endpoint_url,
            stream: Arc::new(Mutex::new(stream_state)),
        })
    }

    /// Send a JSON-RPC request and read the response directly from the SSE stream.
    pub async fn send_request(&self, request_body: &Value) -> Result<Value, String> {
        let body = serde_json::to_string(request_body)
            .map_err(|e| format!("serialize request: {e}"))?;

        let req_id = request_body.get("id").and_then(|v| v.as_u64());

        // POST the request
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

        // Consume and discard the POST response body to avoid blocking the
        // HTTP/2 flow control window (which can prevent the SSE stream from
        // receiving further data from the server).
        let mut resp = resp;
        while let Some(chunk) = resp.chunk().await.map_err(|e| format!("POST body read: {e}"))? {
            let _ = chunk; // discard
        }

        // Notifications have no response
        if req_id.is_none() {
            // Drop the response body without reading (avoids blocking on chunked encoding)
            return Ok(Value::Null);
        }
        let expected_id = req_id.unwrap();

        // Lock the stream and read until matching response
        let mut guard = self.stream.lock().await;
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(SSE_REQUEST_TIMEOUT_SECS);

        loop {
            // Check buffered events
            for event in guard.drain_events() {
                if let Some(response) = try_extract_response(&event.data, expected_id)? {
                    return Ok(response);
                }
            }

            // Read more data with timeout
            let chunk_result = tokio::time::timeout_at(deadline, guard.read_chunk()).await;
            match chunk_result {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => {
                    return Err("SSE stream closed before response arrived".to_string());
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(format!(
                        "MCP SSE response timed out after {}s (request id={})",
                        SSE_REQUEST_TIMEOUT_SECS, expected_id
                    ));
                }
            }
        }
    }
}

/// Try to parse an SSE event's data as a JSON-RPC response matching `expected_id`.
fn try_extract_response(data: &str, expected_id: u64) -> Result<Option<Value>, String> {
    let value: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let resp_id = value.get("id").and_then(|v| v.as_u64());
    if resp_id != Some(expected_id) {
        return Ok(None);
    }

    if let Some(error) = value.get("error") {
        return Err(format!("MCP error: {error}"));
    }

    Ok(value.get("result").cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_events_single() {
        let raw = "event: endpoint\ndata: /message?sessionId=abc\n\n";
        let events = parse_sse_events(raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "endpoint");
        assert_eq!(events[0].data, "/message?sessionId=abc");
    }

    #[test]
    fn test_parse_sse_events_multiple() {
        let raw = "event: endpoint\ndata: /msg\n\nevent: message\ndata: hello\n\n";
        let events = parse_sse_events(raw);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "endpoint");
        assert_eq!(events[1].event_type, "message");
    }

    #[test]
    fn test_parse_sse_events_default_type() {
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
    fn test_parse_sse_events_empty() {
        assert!(parse_sse_events("").is_empty());
        assert!(parse_sse_events("\n\n").is_empty());
    }

    #[test]
    fn test_resolve_endpoint_url_relative() {
        let base = "https://api.example.com/sse?token=abc";
        let result = resolve_endpoint_url(base, "/message?sessionId=123").unwrap();
        assert_eq!(result, "https://api.example.com/message?sessionId=123");
    }

    #[test]
    fn test_resolve_endpoint_url_absolute() {
        let base = "https://api.example.com/sse";
        let result = resolve_endpoint_url(base, "https://other.example.com/msg").unwrap();
        assert_eq!(result, "https://other.example.com/msg");
    }

    #[test]
    fn test_resolve_endpoint_url_invalid_base() {
        assert!(resolve_endpoint_url("not a url", "/path").is_err());
    }

    #[test]
    fn test_try_extract_response_matching() {
        let data = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        let result = try_extract_response(data, 1).unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().get("tools").is_some());
    }

    #[test]
    fn test_try_extract_response_wrong_id() {
        let data = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        let result = try_extract_response(data, 2).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_try_extract_response_error() {
        let data = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"bad"}}"#;
        let result = try_extract_response(data, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_try_extract_response_not_json() {
        let result = try_extract_response("not json", 1).unwrap();
        assert!(result.is_none());
    }
}
