use crate::domain::types::grep::{GrepFileMatch, GrepInput, GrepOutputMode, GrepResult};
use crate::domain::types::support::Match;
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use super::constants::{GREP_INDEX_HEADER_RESERVE_CHARS, GREP_INDEX_TEXT_BUDGET_CHARS};
use super::process_cleanup::terminate_process_tree;

pub struct GrepTool;

#[async_trait]
impl TypedTool for GrepTool {
    type Output = GrepResult;
    fn name(&self) -> &str {
        "Grep"
    }
    fn description(&self) -> &str {
        share::i18n::tools::filesystem::grep("en")
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

        let index_mode = args.output_mode == GrepOutputMode::FilesWithMatches;
        let mut search_command = if is_rg_available().await {
            let mut rg_command = Command::new("rg");
            if index_mode {
                // 索引模式只要「文件 + 计数」：`rg -c` 输出 `path:count`。
                rg_command.arg("-c").arg("--no-heading");
            } else {
                rg_command.arg("-n").arg("-H").arg("--no-heading");
            }
            rg_command.arg(pattern);
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
            let mode_flag = if index_mode { "-rc" } else { "-rn" };
            grep_command.arg(mode_flag).arg(pattern).arg(&search_path);
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
            Err(error) => {
                let message = utils::describe_cwd_gone_failure(&error, &workspace_root, "搜索命令")
                    .unwrap_or_else(|| format!("Search failed: {error}"));
                return TypedToolResult::error(message);
            }
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
                            query: pattern.to_string(),
                            ..GrepResult::default()
                        },
                    )
                } else if index_mode {
                    build_index_result(&stdout, &workspace_root, args.head_limit, pattern)
                } else {
                    build_content_result(&stdout, args.head_limit, pattern)
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

/// 索引模式：把 `path:count` 输出整理为「文件 + 匹配数」索引。
///
/// 排序按匹配数降序（最相关在前）；路径相对 workspace root 以节省字符预算；
/// 输出受 [`GREP_INDEX_TEXT_BUDGET_CHARS`] 约束——超出时截断文件列表并给出
/// 可操作的收窄提示，保证索引始终落在通用落盘阈值之内、完整可见。
fn build_index_result(
    stdout: &str,
    workspace_root: &Path,
    head_limit: Option<u32>,
    pattern: &str,
) -> TypedToolResult<GrepResult> {
    let mut hits: Vec<(String, u64)> = stdout
        .lines()
        .filter_map(|line| {
            let (path, count) = line.rsplit_once(':')?;
            let match_count = count.parse::<u64>().ok()?;
            Some((path.to_string(), match_count))
        })
        .collect();
    hits.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let total_files = hits.len() as u64;
    let total_matches: u64 = hits.iter().map(|(_, count)| count).sum();

    let file_limit = head_limit.map(|n| n as usize).unwrap_or(usize::MAX);
    let body_budget = GREP_INDEX_TEXT_BUDGET_CHARS.saturating_sub(GREP_INDEX_HEADER_RESERVE_CHARS);
    let mut used_chars = 0usize;
    let mut files: Vec<GrepFileMatch> = Vec::new();
    for (path, match_count) in hits.into_iter().take(file_limit) {
        let display_path = Path::new(&path)
            .strip_prefix(workspace_root)
            .map(|relative| relative.to_string_lossy().to_string())
            .unwrap_or(path);
        let line_chars = display_path.chars().count() + 2 + match_count.to_string().len();
        if used_chars + line_chars + 1 > body_budget {
            break;
        }
        used_chars += line_chars + 1;
        files.push(GrepFileMatch {
            file_path: display_path,
            match_count,
        });
    }

    let shown_files = files.len() as u64;
    let header = if shown_files < total_files {
        format!(
            "Found {total_matches} matches in {total_files} files (showing top {shown_files} by match count); refine pattern or pass glob/path to narrow down"
        )
    } else {
        format!("Found {total_matches} matches in {total_files} files")
    };
    let body = files
        .iter()
        .map(|file| format!("{}: {}", file.file_path, file.match_count))
        .collect::<Vec<_>>()
        .join("\n");
    let text = if body.is_empty() {
        header
    } else {
        format!("{header}\n\n{body}")
    };
    TypedToolResult::success(
        text,
        GrepResult {
            matches: Vec::new(),
            total_matches,
            shown: shown_files,
            query: pattern.to_string(),
            files,
            total_files,
        },
    )
}

/// 逐行内容模式：保留既有 `path:line: text` 语义与 head_limit 截断提示。
fn build_content_result(
    stdout: &str,
    head_limit: Option<u32>,
    pattern: &str,
) -> TypedToolResult<GrepResult> {
    let all_lines: Vec<&str> = stdout.lines().collect();
    let actual_total = all_lines.len();
    let limit = head_limit
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
            files: Vec::new(),
            total_files: 0,
        },
    )
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
