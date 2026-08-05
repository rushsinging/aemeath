use crate::domain::{ProgressSink, ToolProgressEvent};
use std::sync::Arc;
use tokio::process::{ChildStderr, ChildStdout};

use super::cwd::CWD_MARKER;

/// Maximum bytes to capture from a single pipe (stdout or stderr).
/// Prevents OOM from commands that produce massive output.
pub(super) const MAX_CAPTURE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

pub(super) async fn read_stdout(
    mut stdout_pipe: Option<ChildStdout>,
    progress_tx: Option<Arc<dyn ProgressSink>>,
) -> Vec<u8> {
    let mut buf = Vec::new();
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
        ($tx:expr, $text:expr) => {{
            if !$text.is_empty() {
                $tx.emit_tool_stream(ToolProgressEvent {
                    text: $text.to_string(),
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
                        let new_data = String::from_utf8_lossy(&tmp[..n]);
                        let mut combined = std::mem::take(&mut suffix_carry);
                        combined.push_str(&new_data);

                        // marker 前的 `\n` 是 bash.rs 包装脚本的分隔符（printf '\n{MARKER}...'），
                        // 非命令输出，与 cwd.rs 的 trim_end_matches('\n') 保持一致。
                        match combined.find(CWD_MARKER) {
                            Some(pos) => {
                                let display_text =
                                    if pos > 0 && combined.as_bytes()[pos - 1] == b'\n' {
                                        &combined[..pos - 1]
                                    } else {
                                        &combined[..pos]
                                    };
                                // 命令结束：flush marker 前的全部内容（含 carry，跨 chunk 收尾）。
                                line_buf.push_str(display_text);
                                suffix_carry.clear();
                            }
                            None => {
                                // marker 未出现：carry 的数据已在上次 read 处理过，
                                // 只处理新数据，避免重复发送产生重复行/空行。
                                line_buf.push_str(&new_data);
                                let carry_len = marker_len.saturating_sub(1).min(combined.len());
                                suffix_carry =
                                    share::string_idx::slice_tail(&combined, carry_len).to_string();
                            }
                        }

                        while let Some(nl) = line_buf.find('\n') {
                            let line: String = line_buf.drain(..=nl).collect();
                            send_progress!(tx, line);
                        }
                        if line_buf.len() > MAX_STREAM_LINE {
                            let flush: String = std::mem::take(&mut line_buf);
                            send_progress!(tx, flush);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }
    if let Some(tx) = &progress_tx {
        if !line_buf.is_empty() {
            send_progress!(tx, line_buf);
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
