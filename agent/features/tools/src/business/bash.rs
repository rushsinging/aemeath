mod safety;

use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use safety::{check_command_safety, check_shell_injection};

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

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> ToolResult {
        let command = match input.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": "missing required parameter: command"
                }))
            }
        };
        if let Some(reason) = check_command_safety(command) {
            if !ctx.allow_all {
                return ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": format!("Destructive command blocked ({reason}): {command}\nIf you really need to run this, ask the user to execute it manually.")
                }));
            }
        }
        // Check for shell injection patterns (skip when allow_all is set)
        if !ctx.allow_all {
            if let Some(reason) = check_shell_injection(command) {
                return ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": format!("Shell injection pattern blocked ({reason}): {command}\nUse separate Bash calls instead.")
                }));
            }
        }
        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        let path_base = ctx.workspace_read().current_path_base();
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
            Err(e) => {
                return ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": format!("failed to execute: {e}")
                }))
            }
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
                return ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": "[interrupted by user]"
                }));
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
                    if let Err(e) = ctx.workspace_control().set_cwd(new_path_base) {
                        return ToolResult::error_json(serde_json::json!({
                            "status": "error",
                            "message": e.to_string()
                        }));
                    }
                }
                let stderr = String::from_utf8_lossy(&stderr);
                let exit_code = status.code().unwrap_or(-1);
                let mut data = serde_json::json!({
                    "stdout": stdout.to_string(),
                    "exit_code": exit_code
                });
                if !stderr.is_empty() {
                    data["stderr"] = serde_json::Value::String(stderr.to_string());
                }
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
                let mut result_json = serde_json::json!({
                    "status": if status.success() { "success" } else { "error" },
                    "message": if status.success() {
                        "Command executed successfully".to_string()
                    } else {
                        format!("Command failed with exit code {exit_code}")
                    },
                    "data": data,
                });
                if !display.is_empty() {
                    result_json["display"] = serde_json::Value::String(display);
                }
                if status.success() {
                    ToolResult::success_json(result_json)
                } else {
                    ToolResult::error_json(result_json)
                }
            }
            Ok(Err(e)) => ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": format!("failed to execute: {e}")
            })),
            Err(_) => {
                let _ = child.kill().await;
                ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": format!("command timed out after {timeout_ms}ms")
                }))
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
    use crate::api::Tool;
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
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
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
        assert_eq!(ws.current_root(), worktree);
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
            allow_all: true,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
        };

        let result = BashTool
            .call(json!({ "command": "echo hello_world_12345" }), &ctx)
            .await;

        assert!(!result.is_error);
        // output 应包含 stdout 内容，而非 "Command executed successfully"
        assert!(
            result.output.contains("hello_world_12345"),
            "output 应包含 stdout，实际: {}",
            result.output
        );
        assert!(
            !result.output.contains("Command executed successfully"),
            "output 不应是元信息 'Command executed successfully'，实际: {}",
            result.output
        );
        // content 中应有 display 字段
        assert_eq!(
            result.content["display"].as_str(),
            Some("hello_world_12345"),
            "content[display] 应为 stdout 内容"
        );
    }
}
