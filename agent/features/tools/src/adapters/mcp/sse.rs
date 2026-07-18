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

use crate::adapters::mcp::sse_stream::SseReadStream;
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
///
/// Some SSE servers (e.g. z.ai) split large responses across multiple chunks
/// with long pauses between them. A shorter timeout with retries via stale
/// response acceptance is more reliable than a single long timeout.
const SSE_REQUEST_TIMEOUT_SECS: u64 = 15;

/// Build a reqwest client with optional custom headers and sane timeouts.
pub fn build_http_client(headers: &HashMap<String, String>) -> Result<Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(SSE_CONNECT_TIMEOUT_SECS))
        .pool_max_idle_per_host(0); // disable connection pooling
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
    /// Client used for the SSE GET stream (must NOT be shared with POST).
    /// Kept alive to ensure the SSE TCP connection stays open.
    #[allow(dead_code)]
    stream_client: Client,
    /// Separate client for POST requests — avoids HTTP/2 flow control issues
    /// where consuming the POST body interferes with the SSE GET stream.
    post_client: Client,
    endpoint_url: String,
    stream: Arc<Mutex<SseReadStream>>,
}

impl SseTransport {
    /// Connect to an SSE MCP server.
    pub async fn connect(sse_url: &str, headers: &HashMap<String, String>) -> Result<Self, String> {
        let stream_client = build_http_client(headers)?;
        let post_client = build_http_client(headers)?;

        log::info!(target: crate::LOG_TARGET, "[MCP:SSE] connecting to {sse_url}");

        let response = stream_client
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
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(SSE_CONNECT_TIMEOUT_SECS);

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
                    return Err("SSE stream ended before sending 'endpoint' event".to_string());
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

        log::info!(target: crate::LOG_TARGET, "[MCP:SSE] connected, endpoint={endpoint_url}");

        Ok(Self {
            stream_client,
            post_client,
            endpoint_url,
            stream: Arc::new(Mutex::new(stream_state)),
        })
    }

    /// Send a JSON-RPC request and read the response directly from the SSE stream.
    pub async fn send_request(&self, request_body: &Value) -> Result<Value, String> {
        let body =
            serde_json::to_string(request_body).map_err(|e| format!("serialize request: {e}"))?;

        let req_id = request_body.get("id").and_then(|v| v.as_u64());

        // POST the request using the separate client
        let resp = self
            .post_client
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
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("POST body read: {e}"))?
        {
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
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(SSE_REQUEST_TIMEOUT_SECS);

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
///
/// Accepts responses where `resp_id == expected_id` (exact match) or
/// `resp_id < expected_id` (stale response from a previous timed-out request).
fn try_extract_response(data: &str, expected_id: u64) -> Result<Option<Value>, String> {
    let value: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let resp_id = value.get("id").and_then(|v| v.as_u64());

    // Accept exact match or stale response from a previous attempt
    match resp_id {
        Some(rid) if rid == expected_id => {}
        Some(rid) if rid < expected_id => {
            log::info!(target: crate::LOG_TARGET, "[MCP:SSE] accepting stale response id={rid} (expected {expected_id})");
        }
        _ => return Ok(None),
    }

    if let Some(error) = value.get("error") {
        return Err(format!("MCP error: {error}"));
    }

    Ok(value.get("result").cloned())
}

#[cfg(test)]
#[path = "sse_tests.rs"]
mod tests;
