use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::grep::{GrepInput, GrepResult};
use share::tool::types::support::Match;
use share::tool::{PathAccess, PathKind};
use std::path::PathBuf;
use tokio::process::Command;

pub struct GrepTool;

const PATH_ACCESS: [PathAccess; 1] = [PathAccess {
    field: "path",
    kind: PathKind::SearchDir,
}];

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
        use share::tool::types::ToolSchema;
        GrepInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        GrepResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    fn path_accesses(&self) -> &'static [PathAccess] {
        &PATH_ACCESS
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
        // Path has already been validated and normalised by PolicyEngine
        let workspace_root = ctx.workspace_read().current_workspace_root();
        let search_path = match args.path.as_deref() {
            Some(p) => std::path::PathBuf::from(p),
            None => workspace_root.clone(),
        };
        let glob_filter = args.glob.as_deref();

        let output = if is_rg_available().await {
            let mut cmd = Command::new("rg");
            cmd.arg("-n").arg("-H").arg("--no-heading").arg(pattern);
            if let Some(g) = glob_filter {
                cmd.arg("--glob").arg(g);
            }
            cmd.arg(&search_path)
                .current_dir(&workspace_root)
                .output()
                .await
        } else {
            let mut cmd = Command::new("grep");
            cmd.arg("-rn").arg(pattern).arg(&search_path);
            if let Some(g) = glob_filter {
                cmd.arg("--include").arg(g);
            }
            cmd.current_dir(&workspace_root).output().await
        };

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
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
    Command::new("rg")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "grep_tests.rs"]
mod grep_tests;
