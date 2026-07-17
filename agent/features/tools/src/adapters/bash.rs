mod bash_result;
mod cwd;
mod safety;
mod stream;

#[cfg(test)]
mod tests;

use crate::domain::types::bash::{BashInput, BashResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use crate::LOG_TARGET;
use async_trait::async_trait;
use bash_result::{exit_status_description, preview};
use cwd::{split_stdout_and_cwd, CWD_MARKER};
use safety::{check_command_safety, check_shell_injection};

pub use safety::is_readonly_command;
use serde_json::Value;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::time::{Duration, Instant};
use tokio::process::Command;

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
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::filesystem::bash(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        BashInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
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
            if reason.contains("dedicated file tools") || !ctx.resources.allow_all {
                return TypedToolResult::error(format!("Command blocked ({reason}): {command}\nUse the dedicated tool requested by the error message, or ask the user to execute it manually if this is intentional."));
            }
        }
        // Check for shell injection patterns (skip when allow_all is set)
        if !ctx.resources.allow_all {
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
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        let stdout_handle = tokio::spawn(stream::read_stdout(stdout_pipe, ctx.progress_tx.clone()));
        let stderr_handle = tokio::spawn(stream::read_stderr(stderr_pipe));

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
                    if let Err(e) = ctx.workspace_control().change_directory(new_path_base) {
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
                // #500: 从 workspace 读取当前 path_base（cd 后已更新），追加到 output 末尾
                let current_cwd = ctx.workspace_read().current_path_base();
                let output = if display.is_empty() {
                    if status.success() {
                        format!(
                            "Command executed successfully\n[cwd: {}]",
                            current_cwd.display()
                        )
                    } else {
                        format!(
                            "Command failed: {failure_detail}\n[cwd: {}]",
                            current_cwd.display()
                        )
                    }
                } else {
                    format!("{}\n[cwd: {}]", display, current_cwd.display())
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
