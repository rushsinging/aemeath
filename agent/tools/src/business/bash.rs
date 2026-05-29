mod safety;

use async_trait::async_trait;
use safety::{check_command_safety, check_shell_injection};
use share::tool::{Tool, ToolContext, ToolResult};

pub use safety::is_readonly_command;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

/// Maximum bytes to capture from a single pipe (stdout or stderr).
/// Prevents OOM from commands that produce massive output.
const MAX_CAPTURE_BYTES: usize = 10 * 1024 * 1024; // 10 MB
const CWD_MARKER: &str = "__AEMEATH_CWD__=";

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }
    fn description(&self) -> &str {
        "Executes a given bash command and returns its output.\n\nThe working directory persists between commands, but shell state does not.\n\nIMPORTANT: Avoid using this tool to run `find`, `grep`, `cat`, `head`, `tail`, `sed`, `awk`, or `echo` commands. Instead, use the appropriate dedicated tool:\n\n - File search: Use Glob (NOT find or ls)\n - Content search: Use Grep (NOT grep or rg)\n - Read files: Use Read (NOT cat/head/tail)\n - Edit files: Use Edit (NOT sed/awk)\n - Write files: Use Write (NOT echo >/cat <<EOF)\n\n# Instructions\n - Always quote file paths that contain spaces with double quotes\n - You may specify an optional timeout in milliseconds (up to 600000ms / 10 minutes). Default timeout is 120000ms (2 minutes).\n - When issuing multiple commands, use && to chain them together.\n - For git commands, prefer creating a new commit rather than amending."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The bash command to execute" },
                "timeout": { "type": "integer", "description": "Timeout in milliseconds (default 120000)" }
            },
            "required": ["command"]
        })
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

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let command = match input.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("missing required parameter: command"),
        };
        if let Some(reason) = check_command_safety(command) {
            if !ctx.allow_all {
                return ToolResult::error(format!(
                    "Destructive command blocked ({reason}): {command}\nIf you really need to run this, ask the user to execute it manually."
                ));
            }
        }
        // Check for shell injection patterns (skip when allow_all is set)
        if !ctx.allow_all {
            if let Some(reason) = check_shell_injection(command) {
                return ToolResult::error(format!(
                    "Shell injection pattern blocked ({reason}): {command}\nUse separate Bash calls instead."
                ));
            }
        }
        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        let path_base = ctx.current_path_base();
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
            Err(e) => return ToolResult::error(format!("failed to execute: {e}")),
        };

        // Take stdout/stderr pipes before spawning readers
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();

        let stdout_handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(ref mut pipe) = stdout_pipe {
                let mut tmp = [0u8; 8192];
                loop {
                    match tokio::io::AsyncReadExt::read(pipe, &mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if buf.len() + n <= MAX_CAPTURE_BYTES {
                                buf.extend_from_slice(&tmp[..n]);
                            }
                            // If over limit, keep reading (to drain the pipe) but don't store
                        }
                        Err(_) => break,
                    }
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
        let wait_result = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => {
                let _ = child.kill().await;
                stdout_handle.abort();
                stderr_handle.abort();
                return ToolResult::error("[interrupted by user]".to_string());
            }
            result = tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait()) => {
                result
            }
        };

        let stdout = stdout_handle.await.unwrap_or_default();
        let stderr = stderr_handle.await.unwrap_or_default();

        match wait_result {
            Ok(Ok(status)) => {
                let stdout = String::from_utf8_lossy(&stdout);
                let (stdout, new_path_base) = split_stdout_and_cwd(&stdout);
                if let Some(new_path_base) = new_path_base {
                    ctx.set_working_directory(new_path_base);
                }
                let stderr = String::from_utf8_lossy(&stderr);
                let mut out = String::new();
                if !stdout.is_empty() {
                    out.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str("stderr:\n");
                    out.push_str(&stderr);
                }
                if out.is_empty() {
                    out.push_str("(no output)");
                }
                if status.success() {
                    ToolResult::success(out)
                } else {
                    ToolResult::error(format!(
                        "exit code: {}\n{}",
                        status.code().unwrap_or(-1),
                        out
                    ))
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("failed to execute: {e}")),
            Err(_) => {
                let _ = child.kill().await;
                ToolResult::error(format!("command timed out after {timeout_ms}ms"))
            }
        }
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
    use share::tool::Tool;
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
        let path_base = Arc::new(Mutex::new(workspace.path().to_path_buf()));
        let working_root = Arc::new(Mutex::new(workspace.path().to_path_buf()));
        let ctx = ToolContext {
            cwd: workspace.path().to_path_buf(),
            working_root: Arc::clone(&working_root),
            path_base: Arc::clone(&path_base),
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
            context_stack: Arc::new(Mutex::new(Vec::new())),
        };

        let result = BashTool
            .call(
                json!({ "command": format!("cd {}", worktree.display()) }),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        assert_eq!(*path_base.lock().unwrap(), worktree);
        assert_eq!(*working_root.lock().unwrap(), worktree);
    }
}
