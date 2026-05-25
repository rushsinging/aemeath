//! Enter/Exit Worktree tools
//!
//! These tools allow the agent to switch between git worktree directories
//! while maintaining a context stack for nested worktree navigation.

use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Tool to enter a git worktree directory
pub struct EnterWorktreeTool;

/// Tool to exit the current worktree and restore the previous context
pub struct ExitWorktreeTool;

#[derive(Debug, Clone, Deserialize)]
pub struct EnterWorktreeInput {
    /// worktree 根目录路径（绝对或相对路径）
    pub path: String,
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

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &'static str {
        "EnterWorktree"
    }

    fn description(&self) -> &'static str {
        "进入指定的 git worktree 目录，将当前工作上下文压栈保存。\
         使用场景：当需要在不同分支的 worktree 中工作时，可以切换到目标 worktree \
         进行文件读取、编辑、执行命令等操作，完成后通过 ExitWorktree 恢复原始上下文。\
         注意：不允许嵌套进入，必须先 ExitWorktree 退出当前 worktree 才能进入新的。\
         目标路径必须属于当前 git 仓库的 worktree。"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "worktree 根目录路径（绝对或相对路径）"
                }
            },
            "required": ["path"]
        })
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let args: EnterWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => return ToolResult::error(format!("Invalid input: {}", e)),
        };

        match project::worktree::enter_worktree(ctx, PathBuf::from(&args.path)) {
            Ok(_snapshot) => {
                let working_root = ctx.current_working_root();
                let branch = get_current_branch(&working_root);
                ToolResult::success(format!(
                    "已进入 worktree：{}\n当前分支：{}\n工作根目录：{}",
                    args.path,
                    branch,
                    working_root.display()
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
            // 直接切到指定路径：先 enter，再 pop 栈顶（enter push 了一层）
            match project::worktree::enter_worktree(ctx, PathBuf::from(&path)) {
                Ok(_) => {
                    // 弹出 enter_worktree 刚压入的快照
                    let _ = ctx.context_stack.lock().map(|mut s| s.pop());
                    let working_root = ctx.current_working_root();
                    let branch = get_current_branch(&working_root);
                    ToolResult::success(format!(
                        "已切换到：{}\n当前分支：{}\n工作根目录：{}",
                        path,
                        branch,
                        working_root.display()
                    ))
                }
                Err(e) => ToolResult::error(format!("切换路径失败：{}", e)),
            }
        } else {
            // 恢复上一上下文
            match project::worktree::exit_worktree(ctx) {
                Ok(prev) => {
                    let working_root = ctx.current_working_root();
                    let branch = get_current_branch(&working_root);
                    ToolResult::success(format!(
                        "已退出 worktree，恢复到：{}\n当前分支：{}\n工作根目录：{}",
                        prev.path_base.display(),
                        branch,
                        working_root.display()
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
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&Value::String("path".to_string())));
    }

    #[test]
    fn test_exit_worktree_schema() {
        let tool = ExitWorktreeTool;
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        // required should be empty
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
}
