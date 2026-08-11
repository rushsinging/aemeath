use crate::domain::{ProgressSink, ToolProgressEvent};
use std::sync::Arc;
use tokio::process::{ChildStderr, ChildStdout};

use super::cwd::CWD_MARKER;

/// Maximum bytes to capture from a single pipe (stdout or stderr).
/// Prevents OOM from commands that produce massive output.
pub(super) const MAX_CAPTURE_BYTES: usize = 10 * 1024 * 1024; // 10 MB
const MAX_STREAM_LINE_BYTES: usize = 16 * 1024;

struct ProgressLineBuffer<'a> {
    marker: &'a str,
    pending: String,
    marker_seen: bool,
}

impl<'a> ProgressLineBuffer<'a> {
    fn new(marker: &'a str) -> Self {
        Self {
            marker,
            pending: String::new(),
            marker_seen: false,
        }
    }

    fn push(&mut self, text: &str) -> Vec<String> {
        if self.marker_seen {
            return Vec::new();
        }
        self.pending.push_str(text);
        self.hide_marker_and_suffix();
        self.take_complete_lines()
    }

    fn finish(&mut self) -> Vec<String> {
        if self.marker_seen || self.pending.is_empty() {
            return Vec::new();
        }
        vec![std::mem::take(&mut self.pending)]
    }

    fn hide_marker_and_suffix(&mut self) {
        let Some(marker_index) = self.pending.find(self.marker) else {
            return;
        };
        let visible_end =
            if marker_index > 0 && self.pending.as_bytes().get(marker_index - 1) == Some(&b'\n') {
                marker_index - 1
            } else {
                marker_index
            };
        self.pending.truncate(visible_end);
        self.marker_seen = true;
    }

    fn take_complete_lines(&mut self) -> Vec<String> {
        let mut events = Vec::new();
        loop {
            let split_end = self
                .pending
                .find('\n')
                .map(|newline_index| newline_index + 1)
                .or_else(|| {
                    (self.pending.len() >= MAX_STREAM_LINE_BYTES)
                        .then(|| self.pending.floor_char_boundary(MAX_STREAM_LINE_BYTES))
                });
            let Some(split_end) = split_end else {
                break;
            };
            let text = self.pending[..split_end].to_owned();
            self.pending.drain(..split_end);
            if !text.trim().is_empty() {
                events.push(text);
            }
        }
        events
    }
}

pub(super) async fn read_stdout(
    mut stdout_pipe: Option<ChildStdout>,
    progress_tx: Option<Arc<dyn ProgressSink>>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut progress_buffer = ProgressLineBuffer::new(CWD_MARKER);

    if let Some(ref mut pipe) = stdout_pipe {
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                Ok(0) => break,
                Ok(bytes_read) => {
                    if buf.len() + bytes_read <= MAX_CAPTURE_BYTES {
                        buf.extend_from_slice(&tmp[..bytes_read]); // allow unsafe_text_op: Vec slice (bytes)
                    }
                    // If over limit, keep reading (to drain the pipe) but don't store.
                    if let Some(progress_sink) = &progress_tx {
                        let text = String::from_utf8_lossy(&tmp[..bytes_read]); // allow unsafe_text_op: Vec slice (bytes)
                        for event_text in progress_buffer.push(&text) {
                            progress_sink.emit_tool_stream(ToolProgressEvent { text: event_text });
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }
    if let Some(progress_sink) = &progress_tx {
        for event_text in progress_buffer.finish() {
            progress_sink.emit_tool_stream(ToolProgressEvent { text: event_text });
        }
    }
    buf
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;

pub(super) async fn read_stderr(mut stderr_pipe: Option<ChildStderr>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(ref mut pipe) = stderr_pipe {
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                Ok(0) => break,
                Ok(bytes_read) => {
                    if buf.len() + bytes_read <= MAX_CAPTURE_BYTES {
                        buf.extend_from_slice(&tmp[..bytes_read]); // allow unsafe_text_op: Vec slice (bytes)
                    }
                }
                Err(_) => break,
            }
        }
    }
    buf
}
