use crate::adapters::mcp::sse::SseEvent;
use crate::LOG_TARGET;
use futures_util::StreamExt;
use serde_json::Value;

/// Wrapper holding the boxed SSE byte stream and a leftover buffer.
pub(super) struct SseReadStream {
    pub(super) buffer: String,
    pub(super) stream:
        std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
}

impl SseReadStream {
    /// Read the next chunk from the stream and append to buffer.
    pub(super) async fn read_chunk(&mut self) -> Result<bool, String> {
        match self.stream.next().await {
            Some(Ok(chunk)) => {
                let len = chunk.len();
                self.buffer.push_str(&String::from_utf8_lossy(&chunk));
                log::debug!(target: LOG_TARGET,
                    "[MCP:SSE] read_chunk: {len} bytes, buffer now {} bytes",
                    self.buffer.len()
                );
                Ok(true)
            }
            Some(Err(e)) => {
                log::warn!(target: LOG_TARGET, "[MCP:SSE] read_chunk error: {e}");
                Err(format!("SSE read error: {e}"))
            }
            None => {
                log::info!(target: LOG_TARGET, "[MCP:SSE] read_chunk: stream EOF");
                Ok(false)
            }
        }
    }

    /// Drain all complete SSE events from the buffer, returning them.
    /// Also attempts to parse events that may be missing the trailing \n\n
    /// (some servers don't send the delimiter for the last event before a pause).
    pub(super) fn drain_events(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();
            if let Some(event) = parse_single_event(&block) {
                events.push(event);
            }
        }

        // Fallback: if remaining buffer has data but no \n\n delimiter,
        // try to parse the event anyway by directly parsing JSON.
        // Handles servers (e.g. z.ai) that omit the trailing \n\n.
        if !self.buffer.is_empty() && self.buffer.contains("data:") {
            if let Some(event) = try_parse_incomplete_event(&self.buffer) {
                log::info!(target: LOG_TARGET,
                    "[MCP:SSE] parsed incomplete event (no trailing \\n\\n), data_len={}",
                    event.data.len()
                );
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
        Some(SseEvent { event_type, data })
    } else {
        None
    }
}

/// Try to parse an SSE event that may be missing the trailing `\n\n`.
///
/// Some servers (notably z.ai) omit the `\n\n` delimiter after the last SSE
/// event, causing the client to buffer the data forever.  We work around this
/// by extracting the `data:` payload and attempting a direct JSON parse.
pub(super) fn try_parse_incomplete_event(buffer: &str) -> Option<SseEvent> {
    // Quick check — must contain "data:"
    let data_start = buffer.find("data:")?;
    let data_content = buffer[data_start + 5..].trim_start();

    // Fast check: does the data end with something that looks like JSON end?
    // (avoids expensive JSON parse attempt on obviously incomplete data)
    let trimmed = data_content.trim_end();
    if !trimmed.ends_with('}') && !trimmed.ends_with(']') {
        return None;
    }

    // Attempt actual JSON parse
    if serde_json::from_str::<Value>(data_content).is_err() {
        return None;
    }

    // JSON is valid — build the event
    let event_part = &buffer[..data_start];
    let event_type = event_part
        .lines()
        .find_map(|line| line.strip_prefix("event:"))
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "message".to_string());

    let data = data_content.to_string();
    log::info!(target: LOG_TARGET,
        "[MCP:SSE] incomplete-fallback: event={event_type} data_len={}",
        data.len()
    );
    Some(SseEvent { event_type, data })
}
