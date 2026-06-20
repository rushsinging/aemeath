mod safety;

use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use crate::LOG_TARGET;
use async_trait::async_trait;
use safety::{check_command_safety, check_shell_injection};
use share::tool::types::bash::{BashInput, BashResult};
use share::tool::{AgentProgressEvent, AgentProgressKind};

pub use safety::is_readonly_command;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::process::Command;

// Unix: 需要从 ExitStatus 获取 signal 信息
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

/// Maximum bytes to capture from a single pipe (stdout or stderr).
/// Prevents OOM from commands that produce massive output.
const MAX_CAPTURE_BYTES: usize = 10 * 1024 * 1024; // 10 MB
const CWD_MARKER: &str = "__AEMEATH_CWD__=";

pub struct BashTool;

#[async_trait]
impl TypedTool for BashTool {
    type Output = BashResult;
    fn name(&self) -> &str {
        "Bash"
    }
    fn description(&self) -> &str {
        "Executes a bash command and returns its output. Working directory persists between calls but shell state does not. Chain commands with &&. Optional timeout parameter (default 120s, max 600s)."
    }
    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        BashInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        BashResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    /// Override: Bash commands may run up to 600s (schema max).
    /// The default 120s outer timeout in agent.rs would kill long-running
    /// commands before the internal per-command timeout fires.
    fn timeout_secs(&self) -> u64 {
        600
    }

    fn is_input_safe(&self, input: &Value) -> bool {
        input
            .get("command")
            .and_then(|v| v.as_str())
            .map(is_readonly_command)
            .unwrap_or(false)
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<BashResult> {
        let args: BashInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let command = args.command.as_str();
        if let Some(reason) = check_command_safety(command) {
            if !ctx.allow_all {
                return TypedToolResult::error(format!("Destructive command blocked ({reason}): {command}\nIf you really need to run this, ask the user to execute it manually."));
            }
        }
        // Check for shell injection patterns (skip when allow_all is set)
        if !ctx.allow_all {
            if let Some(reason) = check_shell_injection(command) {
                return TypedToolResult::error(format!("Shell injection pattern blocked ({reason}): {command}\nUse separate Bash calls instead."));
            }
        }
        let timeout_ms = args.timeout.unwrap_or(120_000);

        let path_base = ctx.workspace_read().current_path_base();
        log::debug!(
            target: LOG_TARGET,
            "executing command: path_base={:?} timeout_ms={} command={:?}",
            path_base, timeout_ms, command
        );
        let script =
            format!("{command}\nstatus=$?\nprintf '\\n{CWD_MARKER}%s\\n' \"$PWD\"\nexit $status");
        let mut child = match Command::new("bash")
            .arg("-c")
            .arg(&script)
            .current_dir(&path_base)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return TypedToolResult::error(format!("failed to execute: {e}")),
        };
        // [DIAG] 记录耗时起点与子进程 PID，便于 #286 / 复现诊断
        let start = Instant::now();
        let child_pid = child.id();

        // Take stdout/stderr pipes before spawning readers
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();

        let progress_tx = ctx.progress_tx.clone();
        let stdout_handle = tokio::spawn(async move {
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

            /// Send `text` as a progress event via `tx` (best-effort).
            /// Strips any trailing CWD marker fragment.
            macro_rules! send_progress {
                ($tx:expr, $seq:expr, $text:expr) => {{
                    if !$text.is_empty() {
                        $seq += 1;
                        // Best-effort: drop chunks if channel is full/closed.
                        let _ = $tx.try_send(AgentProgressEvent {
                            sequence: $seq,
                            kind: AgentProgressKind::Message {
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
                            // Stream stdout chunk to TUI via progress_tx.
                            if let Some(tx) = &progress_tx {
                                // Prepend any carried suffix from the previous chunk,
                                // then take the full combined text for processing.
                                let mut combined = std::mem::take(&mut suffix_carry);
                                combined.push_str(&String::from_utf8_lossy(&tmp[..n]));

                                // Strip CWD marker from the combined text.
                                // Retain a suffix in case the marker is split
                                // across reads.
                                let display_text = match combined.find(CWD_MARKER) {
                                    Some(pos) => &combined[..pos],
                                    None => &combined[..],
                                };

                                // Save the tail as suffix_carry for next iteration
                                // (only if we didn't find a marker — once found,
                                // remaining output after the marker is internal).
                                if !display_text.contains(CWD_MARKER) {
                                    let carry_len =
                                        marker_len.saturating_sub(1).min(display_text.len());
                                    suffix_carry =
                                        share::string_idx::slice_tail(display_text, carry_len)
                                            .to_string();
                                }

                                // Append to line buffer and emit completed lines.
                                line_buf.push_str(display_text);
                                while let Some(nl) = line_buf.find('\n') {
                                    let line: String = line_buf.drain(..=nl).collect();
                                    send_progress!(tx, sequence, line);
                                }
                                // Flush if buffer exceeds the cap even without newline.
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
            // Flush any remaining buffered text after the read loop ends.
            if let Some(tx) = &progress_tx {
                if !line_buf.is_empty() {
                    send_progress!(tx, sequence, line_buf);
                }
            }
            buf
        });
        let stderr_handle = tokio::spawn(async move {
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
        });

        // Race: cancel signal vs timeout vs command completion
        let wait_result: Result<std::process::ExitStatus, std::io::Error> = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => {
                let _ = child.kill().await;
                stdout_handle.abort();
                stderr_handle.abort();
                return TypedToolResult::error("[interrupted by user]");
            }
            result = tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                child.wait(),
            ) => {
                match result {
                    Ok(inner) => inner,
                    // Timeout: kill the child immediately and abort
                    // reader tasks so we don't hang awaiting pipes
                    // that will never reach EOF on their own.
                    Err(_) => {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        stdout_handle.abort();
                        stderr_handle.abort();
                        // Return early — no point awaiting aborted
                        // handles.
                        return TypedToolResult::error(format!("command timed out after {timeout_ms}ms"));
                    }
                }
            }
        };

        let stdout = stdout_handle.await.unwrap_or_default();
        let stderr = stderr_handle.await.unwrap_or_default();

        match wait_result {
            Ok(status) => {
                let stdout = String::from_utf8_lossy(&stdout);
                let (stdout, new_path_base) = split_stdout_and_cwd(&stdout);
                // `cd` 改变了 path_base 时通知 workspace 并记录新值，回传给 LLM（#414）。
                let cd_path_base: Option<std::path::PathBuf> = new_path_base.clone();
                if let Some(new_path_base) = new_path_base {
                    if let Err(e) = ctx.workspace_control().set_path_base(new_path_base) {
                        return TypedToolResult::error(e.to_string());
                    }
                }
                let stderr = String::from_utf8_lossy(&stderr);
                let (exit_code, failure_detail) = exit_status_description(&status);

                // 被信号终止时记 warn 日志，方便诊断 OOM kill / 外部 kill 等
                let elapsed_ms = start.elapsed().as_millis();
                if failure_detail.starts_with("signal") {
                    log::warn!(
                        target: LOG_TARGET,
                        "command terminated by signal: {}, command: {:?}, pid={:?} path_base={:?} elapsed_ms={} stdout_len={} stderr_len={} stdout_preview={:?} stderr_preview={:?}",
                        failure_detail,
                        command,
                        child_pid,
                        path_base,
                        elapsed_ms,
                        stdout.len(),
                        stderr.len(),
                        preview(&stdout),
                        preview(&stderr),
                    );
                } else {
                    log::debug!(
                        target: LOG_TARGET,
                        "command finished: exit_code={}, command: {:?}, pid={:?} path_base={:?} elapsed_ms={} stdout_len={} stderr_len={} stdout_preview={:?} stderr_preview={:?}",
                        exit_code,
                        command,
                        child_pid,
                        path_base,
                        elapsed_ms,
                        stdout.len(),
                        stderr.len(),
                        preview(&stdout),
                        preview(&stderr),
                    );
                }

                let bash_result = BashResult {
                    stdout: stdout.to_string(),
                    stderr: if stderr.is_empty() {
                        String::new()
                    } else {
                        stderr.to_string()
                    },
                    exit_code,
                    #[cfg(unix)]
                    signal: status.signal(),
                    #[cfg(not(unix))]
                    signal: None,
                    path_base: cd_path_base,
                };
                // 构造 TUI 显示文本：stdout + stderr（如有），让 TUI 显示实际命令输出
                // 而非 "Command executed successfully" 这类元信息（display > message 优先级）
                let display = {
                    let mut parts: Vec<&str> = Vec::new();
                    if !stdout.is_empty() {
                        parts.push(stdout.as_str());
                    }
                    if !stderr.is_empty() {
                        parts.push(stderr.as_ref());
                    }
                    parts.join("\n")
                };
                let output = if display.is_empty() {
                    if status.success() {
                        "Command executed successfully".to_string()
                    } else {
                        format!("Command failed: {failure_detail}")
                    }
                } else {
                    display
                };
                if status.success() {
                    TypedToolResult::success(output, bash_result)
                } else {
                    let mut result = TypedToolResult::error(output);
                    result.data = Some(bash_result);
                    result
                }
            }
            Err(e) => {
                // [DIAG] wait 失败时记 warn 日志，便于 #286 诊断
                // stdout/stderr 在此分支仍是 Vec<u8>，先用 lossy 转成字符串
                let stdout_lossy = String::from_utf8_lossy(&stdout);
                let stderr_lossy = String::from_utf8_lossy(&stderr);
                log::warn!(
                    target: LOG_TARGET,
                    "wait_result failed: error={}, command: {:?}, pid={:?} path_base={:?} elapsed_ms={} stdout_len={} stderr_len={} stdout_preview={:?} stderr_preview={:?}",
                    e,
                    command,
                    child_pid,
                    path_base,
                    start.elapsed().as_millis(),
                    stdout.len(),
                    stderr.len(),
                    preview(&stdout_lossy),
                    preview(&stderr_lossy),
                );
                TypedToolResult::error(format!("failed to execute: {e}"))
            }
        }
    }
}

/// 从 ExitStatus 提取 (exit_code, failure_detail)。
///
/// - 正常退出：`exit_code` 为实际码，`failure_detail` 为 `"exit code N"`
/// - 信号终止（Unix）：`exit_code` 为 `-1`，`failure_detail` 为 `"signal N (SIGNAME)"`
/// - 信号终止（非 Unix）：`exit_code` 为 `-1`，`failure_detail` 为 `"unknown (no exit code)"`
fn exit_status_description(status: &std::process::ExitStatus) -> (i32, String) {
    if let Some(code) = status.code() {
        return (code, format!("exit code {code}"));
    }
    // 进程没有正常退出码 → 被信号终止
    #[cfg(unix)]
    {
        let signal = status.signal().unwrap_or(0);
        let sig_name = signal_name(signal);
        (-1, format!("signal {signal} ({sig_name})"))
    }
    #[cfg(not(unix))]
    {
        (-1, "unknown (no exit code)".to_string())
    }
}

/// 将常见 Unix signal 编号映射为可读名称（覆盖最常见值，未知返回 "UNKNOWN"）。
fn signal_name(sig: i32) -> &'static str {
    match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        6 => "SIGABRT",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => "UNKNOWN",
    }
}

/// 截断字符串到 PREVIEW_MAX 字节（按 char boundary），超长时附加截断标记。
/// 用于日志预览，避免大输出把日志刷爆。
const PREVIEW_MAX: usize = 512;
fn preview(s: &str) -> String {
    if s.len() <= PREVIEW_MAX {
        s.to_string()
    } else {
        let cut = s
            .char_indices()
            .take_while(|(i, _)| *i < PREVIEW_MAX)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(PREVIEW_MAX);
        format!("{}...[truncated {} bytes]", &s[..cut], s.len() - cut)
    }
}

fn split_stdout_and_cwd(stdout: &str) -> (String, Option<PathBuf>) {
    let Some(pos) = stdout.rfind(CWD_MARKER) else {
        return (stdout.to_string(), None);
    };
    let before_marker = &stdout[..pos];
    let after_marker = &stdout[pos + CWD_MARKER.len()..];
    let Some(first_line_end) = after_marker.find('\n') else {
        return (stdout.to_string(), None);
    };
    let cwd = after_marker[..first_line_end].trim();
    if cwd.is_empty() {
        return (stdout.to_string(), None);
    }

    let visible_stdout = before_marker.trim_end_matches('\n').to_string();
    (visible_stdout, Some(PathBuf::from(cwd)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio::sync::Semaphore;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn test_bash_persists_cd_for_subsequent_write_path_base() {
        let workspace = tempdir().unwrap();
        let worktree = workspace.path().join(".worktrees/bug35");
        std::fs::create_dir_all(&worktree).unwrap();
        let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
        let ctx = ToolExecutionContext {
            cwd: workspace.path().to_path_buf(),
            workspace: ws.clone(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            lang: "en".to_string(),
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
            registry: None,
        };

        let result = BashTool
            .call(
                json!({ "command": format!("cd {}", worktree.display()) }),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        use project::api::WorkspaceRead;
        assert_eq!(ws.current_path_base(), worktree);
        // working_root 应该保持为原来的 git 仓库根目录，不会因为 cd 到非 git 目录而改变
        assert_eq!(ws.current_root(), workspace.path());
    }

    #[tokio::test]
    async fn test_bash_display_field_contains_stdout_not_message() {
        // 回归：Bash result 的 output 应包含 stdout（通过 display 字段），
        // 而非 "Command executed successfully" 元信息。
        let workspace = tempdir().unwrap();
        let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
        let ctx = ToolExecutionContext {
            cwd: workspace.path().to_path_buf(),
            workspace: ws.clone(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            lang: "en".to_string(),
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
            registry: None,
        };

        let result = BashTool
            .call(json!({ "command": "echo hello_world_12345" }), &ctx)
            .await;

        assert!(!result.is_error);
        // output 应包含 stdout 内容，而非 "Command executed successfully"
        assert!(
            result.text.contains("hello_world_12345"),
            "output 应包含 stdout，实际: {}",
            result.text
        );
        assert!(
            !result.text.contains("Command executed successfully"),
            "output 不应是元信息 'Command executed successfully'，实际: {}",
            result.text
        );
        // data 中应有 stdout 字段
        let data = result.data.expect("应有 data");
        assert_eq!(data.stdout, "hello_world_12345", "data.stdout 应为命令输出");
    }

    #[tokio::test]
    async fn test_bash_streams_stdout_via_progress_tx() {
        use tokio::sync::mpsc;

        let workspace = tempdir().unwrap();
        let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
        let (tx, mut rx) = mpsc::channel::<AgentProgressEvent>(256);
        let ctx = ToolExecutionContext {
            cwd: workspace.path().to_path_buf(),
            workspace: ws.clone(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            lang: "en".to_string(),
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: Some(tx),
            parent_session_id: None,
            registry: None,
        };

        let result = BashTool
            .call(
                json!({ "command": "echo progress_stream_test_marker" }),
                &ctx,
            )
            .await;

        assert!(!result.is_error);

        // Drop ctx (which owns the original Sender) so that once the spawned
        // stdout reader finishes and drops its clone, the channel is fully closed
        // and rx.recv() will return None.
        drop(ctx);

        // Collect all progress events
        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }

        assert!(
            !events.is_empty(),
            "progress_tx should have received at least one event"
        );

        // All collected text fragments concatenated should contain the echoed marker
        let all_text: String = events
            .iter()
            .filter_map(|ev| match &ev.kind {
                AgentProgressKind::Message { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert!(
            all_text.contains("progress_stream_test_marker"),
            "progress events should contain echoed output, got: {:?}",
            events
        );

        // No event should contain the internal CWD marker
        for ev in &events {
            if let AgentProgressKind::Message { text } = &ev.kind {
                assert!(
                    !text.contains("__AEMEATH_CWD__"),
                    "progress event must not contain __AEMEATH_CWD__ marker: {}",
                    text
                );
            }
        }

        // Sequence must be monotonically increasing and > 0
        for ev in &events {
            assert!(
                ev.sequence > 0,
                "progress event sequence must be > 0, got {}",
                ev.sequence
            );
        }
    }

    #[tokio::test]
    async fn test_bash_no_progress_tx_still_works() {
        let workspace = tempdir().unwrap();
        let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
        let ctx = ToolExecutionContext {
            cwd: workspace.path().to_path_buf(),
            workspace: ws.clone(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            lang: "en".to_string(),
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
            registry: None,
        };

        let result = BashTool
            .call(json!({ "command": "echo no_channel_test_98765" }), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(
            result.text.contains("no_channel_test_98765"),
            "output should contain echoed text even without progress_tx, got: {}",
            result.text
        );
    }

    // ---- Issue #286: exit code 映射 + 信号终止诊断 ----

    #[test]
    fn test_exit_status_description_normal_exit() {
        // 正常退出码 → 返回实际码 + "exit code N"
        let status = std::process::Command::new("bash")
            .arg("-c")
            .arg("exit 42")
            .status()
            .unwrap();
        let (code, detail) = exit_status_description(&status);
        assert_eq!(code, 42);
        assert_eq!(detail, "exit code 42");
    }

    #[test]
    fn test_exit_status_description_success() {
        let status = std::process::Command::new("bash")
            .arg("-c")
            .arg("true")
            .status()
            .unwrap();
        let (code, detail) = exit_status_description(&status);
        assert_eq!(code, 0);
        assert_eq!(detail, "exit code 0");
    }

    #[cfg(unix)]
    #[test]
    fn test_exit_status_description_signal_termination() {
        // 信号终止 → exit_code=-1, detail 包含 signal 信息
        let status = std::process::Command::new("bash")
            .arg("-c")
            .arg("kill -9 $$")
            .status()
            .unwrap();
        assert!(
            status.code().is_none(),
            "signal termination should have no exit code"
        );
        let (code, detail) = exit_status_description(&status);
        assert_eq!(code, -1);
        assert!(
            detail.starts_with("signal 9"),
            "detail should indicate SIGKILL, got: {detail}"
        );
        assert!(
            detail.contains("SIGKILL"),
            "detail should include signal name, got: {detail}"
        );
    }

    #[test]
    fn test_signal_name_known_signals() {
        assert_eq!(signal_name(9), "SIGKILL");
        assert_eq!(signal_name(15), "SIGTERM");
        assert_eq!(signal_name(2), "SIGINT");
    }

    #[test]
    fn test_signal_name_unknown_signal() {
        assert_eq!(signal_name(255), "UNKNOWN");
        assert_eq!(signal_name(0), "UNKNOWN");
    }

    #[test]
    fn test_preview_no_truncation() {
        // 短字符串（< PREVIEW_MAX）原样返回
        let s = "short stdout";
        assert_eq!(preview(s), "short stdout");
    }

    #[test]
    fn test_preview_truncation_with_marker() {
        // 长字符串（>= PREVIEW_MAX）按 char boundary 截断，附加截断标记
        let s: String = "a".repeat(PREVIEW_MAX + 100);
        let result = preview(&s);
        assert!(
            result.starts_with(&"a".repeat(PREVIEW_MAX)),
            "should keep first PREVIEW_MAX bytes"
        );
        assert!(
            result.contains("...[truncated"),
            "should include truncation marker"
        );
        assert!(
            result.contains("100 bytes"),
            "should report truncated byte count, got: {result}"
        );
    }

    #[test]
    fn test_preview_respects_utf8_char_boundary() {
        // UTF-8 多字节字符：PREVIEW_MAX 落在多字节字符中间时，必须按 char boundary 截断
        // 汉字 "中" 占 3 字节；构造一个 PREVIEW_MAX = 512 全部由汉字组成的字符串
        let s: String = "中".repeat(PREVIEW_MAX);
        // 中占 3 字节，总长 PREVIEW_MAX * 3 > PREVIEW_MAX，必然触发截断
        // 但 PREVIEW_MAX = 512 不是 3 的倍数，可能在某个字符中间；切到最近的 char boundary
        let result = preview(&s);
        // 不应该 panic
        assert!(
            result.contains("...[truncated"),
            "should include truncation marker"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_bash_command_killed_by_signal_reports_signal_in_message() {
        // 回归 #286：被信号杀死的命令不应只报 "exit code -1"，
        // 而应包含 signal 信息。
        let workspace = tempdir().unwrap();
        let ws = project::api::WorkspaceService::new(workspace.path().to_path_buf());
        let ctx = ToolExecutionContext {
            cwd: workspace.path().to_path_buf(),
            workspace: ws.clone(),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            lang: "en".to_string(),
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
            registry: None,
        };

        let result = BashTool
            .call(json!({ "command": "kill -9 $$" }), &ctx)
            .await;

        assert!(result.is_error);
        // 消息应包含 "signal" 而非无信息的 "exit code -1"
        assert!(
            result.text.contains("signal"),
            "error message should contain signal info, got: {}",
            result.text
        );
        assert!(
            result.text.contains("SIGKILL"),
            "error message should contain signal name, got: {}",
            result.text
        );
    }
}
