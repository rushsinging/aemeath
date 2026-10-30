use super::constants::{MAX_CAPTURE_BYTES, MAX_STREAM_LINE_BYTES};
use crate::domain::{ProgressSink, ToolProgressEvent};
use std::sync::Arc;
use tokio::process::{ChildStderr, ChildStdout};

use super::constants::CWD_MARKER;

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

/// 捕获结果：`capped = true` 表示命中 [`MAX_CAPTURE_BYTES`] 硬顶，
/// 调用方 **MUST** 在给 LLM 的文本里显式标注（NEVER 静默截断）。
#[derive(Default)]
pub(super) struct CapturedOutput {
    pub(super) bytes: Vec<u8>,
    pub(super) capped: bool,
}

pub(super) async fn read_stdout(
    mut stdout_pipe: Option<ChildStdout>,
    progress_tx: Option<Arc<dyn ProgressSink>>,
) -> CapturedOutput {
    let mut buf = Vec::new();
    let mut capped = false;
    let mut progress_buffer = ProgressLineBuffer::new(CWD_MARKER);

    if let Some(ref mut pipe) = stdout_pipe {
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                Ok(0) => break,
                Ok(bytes_read) => {
                    let room = MAX_CAPTURE_BYTES.saturating_sub(buf.len());
                    let take = room.min(bytes_read);
                    buf.extend_from_slice(&tmp[..take]); // allow unsafe_text_op: Vec slice (bytes)
                    if take < bytes_read {
                        capped = true;
                    }
                    // 超限后继续读完（排空管道）但不再存储。
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
    CapturedOutput { bytes: buf, capped }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;

pub(super) async fn read_stderr(mut stderr_pipe: Option<ChildStderr>) -> CapturedOutput {
    let mut buf = Vec::new();
    let mut capped = false;
    if let Some(ref mut pipe) = stderr_pipe {
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                Ok(0) => break,
                Ok(bytes_read) => {
                    let room = MAX_CAPTURE_BYTES.saturating_sub(buf.len());
                    let take = room.min(bytes_read);
                    buf.extend_from_slice(&tmp[..take]); // allow unsafe_text_op: Vec slice (bytes)
                    if take < bytes_read {
                        capped = true;
                    }
                }
                Err(_) => break,
            }
        }
    }
    CapturedOutput { bytes: buf, capped }
}
