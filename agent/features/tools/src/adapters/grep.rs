use crate::domain::types::grep::{GrepInput, GrepResult};
use crate::domain::types::support::Match;
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use tokio::process::Command;

use super::process_cleanup::terminate_process_tree;

pub struct GrepTool;

#[async_trait]
impl TypedTool for GrepTool {
    type Output = GrepResult;
    fn name(&self) -> &str {
        "Grep"
    }
    fn description(&self) -> &str {
        "Search file contents using ripgrep regex syntax. Supports glob file filters."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::filesystem::grep(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        GrepInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        GrepResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn cancellation(&self) -> crate::domain::published_language::CancellationDeclaration {
        crate::domain::published_language::CancellationDeclaration::Cooperative
    }

    async fn call(&self, input: Value, ctx: &ToolExecutionContext) -> TypedToolResult<GrepResult> {
        let args: GrepInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!("invalid input: {e}"),
                        "data": {
                            "matches": [],
                            "match_count": 0
                        }
                    })
                    .to_string(),
                )
            }
        };
        let pattern = args.pattern.as_str();
        let workspace = ctx.workspace_read();
        let workspace_root = workspace.current_workspace_root();
        let search_path = match args.path.as_deref() {
            Some(path) => match workspace.resolve_search_path_authorized(
                std::path::Path::new(path),
                ctx.authorization().allow_outside_workspace,
            ) {
                Ok(path) => path,
                Err(error) => return TypedToolResult::error(error.to_string()),
            },
            None => workspace_root.clone(),
        };
        let glob_filter = args.glob.as_deref();

        let mut search_command = if is_rg_available().await {
            let mut rg_command = Command::new("rg");
            rg_command
                .arg("-n")
                .arg("-H")
                .arg("--no-heading")
                .arg(pattern);
            if let Some(glob_pattern) = glob_filter {
                rg_command.arg("--glob").arg(glob_pattern);
            }
            rg_command
                .arg(&search_path)
                .current_dir(&workspace_root)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);
            if let Err(error) = utils::configure_tokio_noninteractive(&mut rg_command) {
                return TypedToolResult::error(format!("Search isolation failed: {error}"));
            }
            rg_command
        } else {
            let mut grep_command = Command::new("grep");
            grep_command.arg("-rn").arg(pattern).arg(&search_path);
            if let Some(glob_pattern) = glob_filter {
                grep_command.arg("--include").arg(glob_pattern);
            }
            grep_command
                .current_dir(&workspace_root)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);
            if let Err(error) = utils::configure_tokio_noninteractive(&mut grep_command) {
                return TypedToolResult::error(format!("Search isolation failed: {error}"));
            }
            grep_command
        };
        let mut search_child = match search_command.spawn() {
            Ok(child) => child,
            Err(error) => return TypedToolResult::error(format!("Search failed: {error}")),
        };
        let cancellation = ctx.cancellation();
        let search_started = std::time::Instant::now();
        let stdout_pipe = search_child.stdout.take();
        let stderr_pipe = search_child.stderr.take();
        let stdout_reader = tokio::spawn(read_pipe_to_end(stdout_pipe));
        let stderr_reader = tokio::spawn(read_pipe_to_end(stderr_pipe));
        let output = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "grep observed cancellation: pattern={pattern:?} pid={:?} elapsed_ms={}",
                    search_child.id(),
                    search_started.elapsed().as_millis(),
                );
                terminate_process_tree(&mut search_child).await;
                log::debug!(
                    target: crate::LOG_TARGET,
                    "grep cancellation cleanup completed: pattern={pattern:?} pid={:?} elapsed_ms={}",
                    search_child.id(),
                    search_started.elapsed().as_millis(),
                );
                return TypedToolResult::error("Search cancelled by user");
            }
            joined = async {
                let status = search_child.wait().await;
                let stdout_bytes = stdout_reader.await.unwrap_or_default();
                let stderr_bytes = stderr_reader.await.unwrap_or_default();
                status.map(|_exit_status| (stdout_bytes, stderr_bytes))
            } => joined.map(|(stdout_bytes, _stderr_bytes)| stdout_bytes),
        };

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out);
                if stdout.is_empty() {
                    TypedToolResult::success(
                        "No matches found",
                        GrepResult {
                            matches: vec![],
                            total_matches: 0,
                            shown: 0,
                            query: pattern.to_string(),
                        },
                    )
                } else {
                    let all_lines: Vec<&str> = stdout.lines().collect();
                    let actual_total = all_lines.len();
                    let limit = args
                        .head_limit
                        .map(|n| n as usize)
                        .unwrap_or(usize::MAX)
                        .min(actual_total);
                    let lines: Vec<&str> = all_lines.iter().take(limit).copied().collect();
                    let parsed_matches: Vec<Match> = lines
                        .iter()
                        .filter_map(|line| {
                            let mut parts = line.splitn(3, ':');
                            let file = parts.next()?;
                            let line_num = parts.next()?.parse::<u64>().ok()?;
                            let text = parts.next().unwrap_or("").to_string();
                            Some(Match {
                                file_path: PathBuf::from(file),
                                line_number: line_num,
                                line: text,
                            })
                        })
                        .collect();
                    let shown = parsed_matches.len() as u64;
                    let query = pattern.to_string();
                    let body = parsed_matches
                        .iter()
                        .map(|m| format!("{}:{}: {}", m.file_path.display(), m.line_number, m.line))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let text = if (shown as usize) < actual_total {
                        format!(
                            "Found {} matches (showing first {})\n\n{}",
                            actual_total, shown, body
                        )
                    } else {
                        format!("Found {} matches\n\n{}", shown, body)
                    };
                    TypedToolResult::success(
                        text,
                        GrepResult {
                            matches: parsed_matches,
                            total_matches: actual_total as u64,
                            shown,
                            query,
                        },
                    )
                }
            }
            Err(e) => TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": format!("Search failed: {e}"),
                    "data": {
                        "matches": [],
                        "match_count": 0
                    }
                })
                .to_string(),
            ),
        }
    }
}

async fn is_rg_available() -> bool {
    let mut command = Command::new("rg");
    command.arg("--version");
    if utils::configure_tokio_noninteractive(&mut command).is_err() {
        return false;
    }
    command
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 读空子进程输出管道直至 EOF；进程被终止后写端关闭，reader 自然返回。
async fn read_pipe_to_end<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    if let Some(mut reader) = pipe {
        let _ = reader.read_to_end(&mut bytes).await;
    }
    bytes
}

#[cfg(test)]
#[path = "grep_tests.rs"]
mod grep_tests;
