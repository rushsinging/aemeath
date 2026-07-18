use crate::domain::{AgentProgressEvent, AgentProgressKind};
use tokio::process::{ChildStderr, ChildStdout};

use super::cwd::CWD_MARKER;

/// Maximum bytes to capture from a single pipe (stdout or stderr).
/// Prevents OOM from commands that produce massive output.
pub(super) const MAX_CAPTURE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

pub(super) async fn read_stdout(
    mut stdout_pipe: Option<ChildStdout>,
    progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut sequence: usize = 0;
    // Line-buffer for coalescing: accumulate partial lines and emit at
    // line boundaries (or when the buffer reaches MAX_STREAM_LINE bytes).
    // This drastically reduces the number of progress events vs per-read
    // sending, mitigating channel pressure and chunk loss.
    let mut line_buf = String::new();
    // Suffix buffer for robust CWD marker detection across chunk splits.
    // The marker "__AEMEATH_CWD__=" is 16 bytes; retaining the last 15
    // bytes of each chunk lets us detect a marker split between reads.
    let marker_len = CWD_MARKER.len();
    let mut suffix_carry = String::new();
    const MAX_STREAM_LINE: usize = 16 * 1024;

    macro_rules! send_progress {
        ($tx:expr, $seq:expr, $text:expr) => {{
            if !$text.is_empty() {
                $seq += 1;
                // Best-effort: drop chunks if channel is full/closed.
                let _ = $tx.try_send(AgentProgressEvent {
                    sequence: $seq,
                    kind: AgentProgressKind::ToolOutput {
                        tool_name: "Bash".to_string(),
                        text: $text.to_string(),
                    },
                });
            }
        }};
    }

    if let Some(ref mut pipe) = stdout_pipe {
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                Ok(0) => break,
                Ok(n) => {
                    if buf.len() + n <= MAX_CAPTURE_BYTES {
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    // If over limit, keep reading (to drain the pipe) but don't store.
                    if let Some(tx) = &progress_tx {
                        let mut combined = std::mem::take(&mut suffix_carry);
                        combined.push_str(&String::from_utf8_lossy(&tmp[..n]));

                        let display_text = match combined.find(CWD_MARKER) {
                            Some(pos) => &combined[..pos],
                            None => &combined[..],
                        };

                        if !display_text.contains(CWD_MARKER) {
                            let carry_len = marker_len.saturating_sub(1).min(display_text.len());
                            suffix_carry =
                                share::string_idx::slice_tail(display_text, carry_len).to_string();
                        }

                        line_buf.push_str(display_text);
                        while let Some(nl) = line_buf.find('\n') {
                            let line: String = line_buf.drain(..=nl).collect();
                            send_progress!(tx, sequence, line);
                        }
                        if line_buf.len() > MAX_STREAM_LINE {
                            let flush: String = std::mem::take(&mut line_buf);
                            send_progress!(tx, sequence, flush);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }
    if let Some(tx) = &progress_tx {
        if !line_buf.is_empty() {
            send_progress!(tx, sequence, line_buf);
        }
    }
    buf
}

pub(super) async fn read_stderr(mut stderr_pipe: Option<ChildStderr>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(ref mut pipe) = stderr_pipe {
        let mut tmp = [0u8; 8192];
        loop {
            match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                Ok(0) => break,
                Ok(n) => {
                    if buf.len() + n <= MAX_CAPTURE_BYTES {
                        buf.extend_from_slice(&tmp[..n]);
                    }
                }
                Err(_) => break,
            }
        }
    }
    buf
}
