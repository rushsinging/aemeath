//! Enter/Exit Worktree tools
//!
//! These tools allow the agent to switch between git worktree directories
//! while maintaining a context stack for nested worktree navigation.

use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use share::tool::types::enter_worktree::EnterWorktreeResult;
use share::tool::types::exit_worktree::ExitWorktreeResult;
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

/// 获取当前分支名；detached HEAD / 无法获取时返回 "(unknown)"。
/// git 调用收敛在 project 的 `GitCli`（GitWorktreeOps port）。
fn get_current_branch(dir: &Path) -> String {
    project::api::GitWorktreeOps::current_branch(&project::api::GitCli, dir)
        .ok()
        .flatten()
        .unwrap_or_else(|| "(unknown)".to_string())
}

fn workspace_context_payload(
    headline: &str,
    branch: &str,
    path_base: &Path,
    working_root: &Path,
) -> Value {
    serde_json::json!({
        "status": "success",
        "message": headline,
        "branch": branch,
        "path_base": path_base.display().to_string(),
        "working_root": working_root.display().to_string(),
        "guidance": [
            "后续 Read/Edit/Write/Glob/Grep/Bash 请优先使用相对路径。",
            "如果必须使用绝对路径，必须位于当前 working_root 下。",
            "不要继续使用进入 worktree 前的 checkout/main workspace 绝对路径。"
        ]
    })
}

#[async_trait]
impl TypedTool for EnterWorktreeTool {
    type Output = EnterWorktreeResult;
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

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<EnterWorktreeResult> {
        let args: EnterWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => {
                return TypedToolResult::error_value(
                    serde_json::json!({"status": "error", "message": format!("Invalid input: {}", e)}),
                )
            }
        };

        let display_target = args.path.clone().unwrap_or_else(|| {
            args.branch
                .clone()
                .map(|branch| format!("branch {branch}"))
                .unwrap_or_else(|| "未指定目标".to_string())
        });

        match ctx
            .workspace_control()
            .enter(args.path.as_ref().map(PathBuf::from), args.branch.clone())
        {
            Ok(_frame) => {
                let path_base = ctx.workspace_read().current_path_base();
                let working_root = ctx.workspace_read().current_root();
                let branch = get_current_branch(&working_root);
                let headline = format!("已进入 worktree：{}", display_target);
                TypedToolResult::success_value(workspace_context_payload(
                    &headline,
                    &branch,
                    &path_base,
                    &working_root,
                ))
            }
            Err(e) => TypedToolResult::error_value(
                serde_json::json!({"status": "error", "message": format!("进入 worktree 失败：{}", e)}),
            ),
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
impl TypedTool for ExitWorktreeTool {
    type Output = ExitWorktreeResult;
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

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<ExitWorktreeResult> {
        let args: ExitWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => {
                return TypedToolResult::error_value(
                    serde_json::json!({"status": "error", "message": format!("Invalid input: {}", e)}),
                )
            }
        };

        if let Some(path) = args.path {
            // 直接切到指定路径：校验存在性 + 同源，不污染上下文栈（不留多余栈帧）。
            match ctx.workspace_control().switch_to(PathBuf::from(&path)) {
                Ok(()) => {
                    let path_base = ctx.workspace_read().current_path_base();
                    let working_root = ctx.workspace_read().current_root();
                    let branch = get_current_branch(&working_root);
                    let headline = format!("已切换到：{}", path);
                    TypedToolResult::success_value(workspace_context_payload(
                        &headline,
                        &branch,
                        &path_base,
                        &working_root,
                    ))
                }
                Err(e) => TypedToolResult::error_value(
                    serde_json::json!({"status": "error", "message": format!("切换路径失败：{}", e)}),
                ),
            }
        } else {
            // 恢复上一上下文
            match ctx.workspace_control().exit() {
                Ok(prev) => {
                    let path_base = ctx.workspace_read().current_path_base();
                    let working_root = ctx.workspace_read().current_root();
                    let branch = get_current_branch(&working_root);
                    let headline = format!("已退出 worktree，恢复到：{}", prev.path_base.display());
                    TypedToolResult::success_value(workspace_context_payload(
                        &headline,
                        &branch,
                        &path_base,
                        &working_root,
                    ))
                }
                Err(e) => TypedToolResult::error_value(
                    serde_json::json!({"status": "error", "message": format!("退出 worktree 失败：{}", e)}),
                ),
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
    fn test_workspace_context_payload_includes_path_base_and_working_root() {
        let payload = workspace_context_payload(
            "已进入 worktree：/repo/.worktrees/feature",
            "feature",
            Path::new("/repo/.worktrees/feature/subdir"),
            Path::new("/repo/.worktrees/feature"),
        );

        assert_eq!(
            payload["message"],
            "已进入 worktree：/repo/.worktrees/feature"
        );
        assert_eq!(payload["branch"], "feature");
        assert_eq!(payload["path_base"], "/repo/.worktrees/feature/subdir");
        assert_eq!(payload["working_root"], "/repo/.worktrees/feature");
        assert!(payload["guidance"][0]
            .as_str()
            .unwrap()
            .contains("后续 Read/Edit/Write/Glob/Grep/Bash"));
    }
}
