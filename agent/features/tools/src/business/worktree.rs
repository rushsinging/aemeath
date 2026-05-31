//! Enter/Exit Worktree tools
//!
//! These tools allow the agent to switch between git worktree directories
//! while maintaining a context stack for nested worktree navigation.

use crate::api::WorktreeContextExt;
use crate::api::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use project::api as worktree_ops;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Tool to enter a git worktree directory
pub struct EnterWorktreeTool;

/// Tool to exit the current worktree and restore the previous context
pub struct ExitWorktreeTool;

#[derive(Debug, Clone, Deserialize)]
pub struct EnterWorktreeInput {
    /// worktree 根目录路径（绝对或相对路径）；省略时从 branch 推导
    #[serde(default)]
    pub path: Option<String>,
    /// 目标路径不存在时创建的新分支名；path 省略时必填
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExitWorktreeInput {
    /// 可选：直接切回指定路径，忽略上下文栈
    #[serde(default)]
    pub path: Option<String>,
}

/// 用 `git rev-parse --abbrev-ref HEAD` 获取当前分支名
fn get_current_branch(dir: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "(unknown)".to_string())
}

fn format_workspace_context_result(
    headline: &str,
    branch: &str,
    path_base: &Path,
    working_root: &Path,
) -> String {
    format!(
        "{headline}\n当前分支：{branch}\n当前 path_base：{}\n当前 working_root：{}\n\n后续 Read/Edit/Write/Glob/Grep/Bash 请优先使用相对路径。\n如果必须使用绝对路径，必须位于当前 working_root 下。\n不要继续使用进入 worktree 前的 checkout/main workspace 绝对路径。",
        path_base.display(),
        working_root.display()
    )
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &'static str {
        "EnterWorktree"
    }

    fn description(&self) -> &'static str {
        "进入或创建 git worktree 目录，将当前工作上下文压栈保存。\
           path 可选：省略时从 branch 推导为 .worktrees/<安全分支名>，其中路径分隔符和敏感字符会替换为 -。\
           如果目标路径不存在，本工具会自动基于 main 执行 git worktree add 创建 worktree 后再进入。\
           开 worktree 时必须调用本工具，NEVER 在主 checkout 中用 git checkout -b 或 git switch -c 代替 worktree。\
           使用场景：当需要在不同分支的 worktree 中工作时，可以切换到目标 worktree \
           进行文件读取、编辑、执行命令等操作，完成后通过 ExitWorktree 恢复原始上下文。\
           注意：不允许嵌套进入，必须先 ExitWorktree 退出当前 worktree 才能进入新的。\
           已在非 main 分支不代表已在 worktree；应以本工具返回的 path_base/working_root 为准。"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "可选：worktree 根目录路径（绝对或相对路径）。省略时从 branch 推导为 .worktrees/<安全分支名>"
                },
                "branch": {
                    "type": "string",
                    "description": "可选：目标路径不存在时创建的新分支名；path 省略时必须提供。创建时固定基于 main"
                }
            },
            "required": []
        })
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let args: EnterWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => return ToolResult::error(format!("Invalid input: {}", e)),
        };

        let display_target = args.path.clone().unwrap_or_else(|| {
            args.branch
                .clone()
                .map(|branch| format!("branch {branch}"))
                .unwrap_or_else(|| "未指定目标".to_string())
        });

        let wc = ctx.worktree_working_context();
        match worktree_ops::enter_worktree(
            &wc,
            args.path.as_ref().map(PathBuf::from),
            args.branch.clone(),
        ) {
            Ok(_snapshot) => {
                let path_base = project::api::current_path(&ctx.path_base);
                let working_root = project::api::current_path(&ctx.working_root);
                let branch = get_current_branch(&working_root);
                ToolResult::success(format_workspace_context_result(
                    &format!("已进入 worktree：{}", display_target),
                    &branch,
                    &path_base,
                    &working_root,
                ))
            }
            Err(e) => ToolResult::error(format!("进入 worktree 失败：{}", e)),
        }
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &'static str {
        "ExitWorktree"
    }

    fn description(&self) -> &'static str {
        "退出当前 worktree，恢复进入前的上下文（从上下文栈中弹出）。\
         如果提供了 path 参数，则直接切换到指定路径（等效于 EnterWorktree 后立即 pop 栈顶）。\
         如果没有提供 path 参数，则恢复上一次 EnterWorktree 保存的工作目录。\
         当上下文栈为空时返回错误。"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "可选：直接切回指定路径，忽略上下文栈"
                }
            },
            "required": []
        })
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let args: ExitWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => return ToolResult::error(format!("Invalid input: {}", e)),
        };

        if let Some(path) = args.path {
            let wc = ctx.worktree_working_context();
            // 直接切到指定路径：先 enter，再 pop 栈顶（enter push 了一层）
            match worktree_ops::enter_worktree(&wc, Some(PathBuf::from(&path)), None) {
                Ok(_) => {
                    let _ = wc.context_stack.lock().map(|mut s| s.pop());
                    let path_base = project::api::current_path(&ctx.path_base);
                    let working_root = project::api::current_path(&ctx.working_root);
                    let branch = get_current_branch(&working_root);
                    ToolResult::success(format_workspace_context_result(
                        &format!("已切换到：{}", path),
                        &branch,
                        &path_base,
                        &working_root,
                    ))
                }
                Err(e) => ToolResult::error(format!("切换路径失败：{}", e)),
            }
        } else {
            let wc = ctx.worktree_working_context();
            // 恢复上一上下文
            match worktree_ops::exit_worktree(&wc) {
                Ok(prev) => {
                    let path_base = project::api::current_path(&ctx.path_base);
                    let working_root = project::api::current_path(&ctx.working_root);
                    let branch = get_current_branch(&working_root);
                    ToolResult::success(format_workspace_context_result(
                        &format!("已退出 worktree，恢复到：{}", prev.path_base.display()),
                        &branch,
                        &path_base,
                        &working_root,
                    ))
                }
                Err(e) => ToolResult::error(format!("退出 worktree 失败：{}", e)),
            }
        }
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enter_worktree_schema() {
        let tool = EnterWorktreeTool;
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        assert_eq!(schema["properties"]["branch"]["type"], "string");
        assert!(schema["properties"].get("base").is_none());
        assert!(schema["required"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_exit_worktree_schema() {
        let tool = ExitWorktreeTool;
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        assert!(schema["required"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_enter_worktree_name() {
        assert_eq!(EnterWorktreeTool.name(), "EnterWorktree");
    }

    #[test]
    fn test_exit_worktree_name() {
        assert_eq!(ExitWorktreeTool.name(), "ExitWorktree");
    }

    #[test]
    fn test_enter_worktree_not_read_only() {
        let tool = EnterWorktreeTool;
        assert!(!tool.is_read_only());
    }

    #[test]
    fn test_exit_worktree_not_read_only() {
        let tool = ExitWorktreeTool;
        assert!(!tool.is_read_only());
    }

    #[test]
    fn test_enter_worktree_not_concurrency_safe() {
        let tool = EnterWorktreeTool;
        assert!(!tool.is_concurrency_safe());
    }

    #[test]
    fn test_exit_worktree_not_concurrency_safe() {
        let tool = ExitWorktreeTool;
        assert!(!tool.is_concurrency_safe());
    }

    #[test]
    fn test_format_workspace_context_result_includes_path_base_and_working_root() {
        let text = format_workspace_context_result(
            "已进入 worktree：/repo/.worktrees/feature",
            "feature",
            Path::new("/repo/.worktrees/feature/subdir"),
            Path::new("/repo/.worktrees/feature"),
        );

        assert!(text.contains("当前 path_base：/repo/.worktrees/feature/subdir"));
        assert!(text.contains("当前 working_root：/repo/.worktrees/feature"));
        assert!(text.contains("后续 Read/Edit/Write/Glob/Grep/Bash"));
        assert!(text.contains("不要继续使用进入 worktree 前"));
    }
}
