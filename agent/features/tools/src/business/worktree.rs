//! Enter/Exit Worktree tools
//!
//! These tools allow the agent to switch between git worktree directories
//! while maintaining a context stack for nested worktree navigation.

use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::i18n::tools::worktree as t;
use share::tool::types::enter_worktree::{EnterWorktreeInput, EnterWorktreeResult};
use share::tool::types::exit_worktree::{ExitWorktreeInput, ExitWorktreeResult};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Tool to enter a git worktree directory
pub struct EnterWorktreeTool;

/// Tool to exit the current worktree and restore the previous context
pub struct ExitWorktreeTool;

/// 获取当前分支名；detached HEAD / 无法获取时返回 "(unknown)"。
/// git 调用收敛在 project 的 `GitCli`（GitWorktreeOps port）。
fn get_current_branch(dir: &Path) -> String {
    project::api::GitWorktreeOps::current_branch(&project::api::GitCli, dir)
        .ok()
        .flatten()
        .unwrap_or_else(|| "(unknown)".to_string())
}

fn workspace_context_payload(
    _headline: &str,
    branch: &str,
    path_base: &Path,
    workspace_root: &Path,
    lang: &str,
) -> EnterWorktreeResult {
    EnterWorktreeResult {
        branch: branch.to_string(),
        path_base: path_base.to_path_buf(),
        workspace_root: workspace_root.to_path_buf(),
        guidance: t::enter_guidance(lang).to_string(),
    }
}

#[async_trait]
impl TypedTool for EnterWorktreeTool {
    type Output = EnterWorktreeResult;
    fn name(&self) -> &'static str {
        "EnterWorktree"
    }

    fn description(&self) -> &'static str {
        t::enter_description(share::i18n::DEFAULT_LANG)
    }

    fn description_for(&self, lang: &str) -> Cow<'_, str> {
        Cow::Borrowed(t::enter_description(lang))
    }

    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        EnterWorktreeInput::data_schema()
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<EnterWorktreeResult> {
        let args: EnterWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => {
                return TypedToolResult::error(t::invalid_input_error(&ctx.resources.lang, e))
            }
        };

        let display_target = args.path.clone().unwrap_or_else(|| {
            args.branch
                .clone()
                .map(|branch| format!("branch {branch}"))
                .unwrap_or_else(|| {
                    if ctx.resources.lang == "zh" {
                        "未指定目标".to_string()
                    } else {
                        "(unspecified)".to_string()
                    }
                })
        });

        match ctx
            .workspace_control()
            .enter(args.path.as_ref().map(PathBuf::from), args.branch.clone())
        {
            Ok(_frame) => {
                let path_base = ctx.workspace_read().current_path_base();
                let workspace_root = ctx.workspace_read().current_workspace_root();
                let branch = get_current_branch(&workspace_root);
                let headline = if ctx.resources.lang == "zh" {
                    format!("已进入 worktree：{}", display_target)
                } else {
                    format!("Entered worktree: {}", display_target)
                };
                TypedToolResult::success(
                    headline.clone(),
                    workspace_context_payload(
                        &headline,
                        &branch,
                        &path_base,
                        &workspace_root,
                        &ctx.resources.lang,
                    ),
                )
            }
            Err(e) => TypedToolResult::error(t::enter_error(&ctx.resources.lang, e)),
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
        t::exit_description(share::i18n::DEFAULT_LANG)
    }

    fn description_for(&self, lang: &str) -> Cow<'_, str> {
        Cow::Borrowed(t::exit_description(lang))
    }

    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        ExitWorktreeInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        ExitWorktreeResult::data_schema()
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<ExitWorktreeResult> {
        let args: ExitWorktreeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => {
                return TypedToolResult::error(t::invalid_input_error(&ctx.resources.lang, e))
            }
        };

        if let Some(path) = args.path {
            // 直接切到指定路径：校验存在性 + 同源，不污染上下文栈（不留多余栈帧）。
            match ctx.workspace_control().switch_to(PathBuf::from(&path)) {
                Ok(()) => {
                    let path_base = ctx.workspace_read().current_path_base();
                    let workspace_root = ctx.workspace_read().current_workspace_root();
                    let branch = get_current_branch(&workspace_root);
                    let headline = if ctx.resources.lang == "zh" {
                        format!("已切换到：{}", path)
                    } else {
                        format!("Switched to: {}", path)
                    };
                    TypedToolResult::success(
                        headline.clone(),
                        ExitWorktreeResult {
                            branch: branch.clone(),
                            path_base: path_base.clone(),
                            workspace_root: workspace_root.clone(),
                            guidance: t::switch_guidance(&ctx.resources.lang, &path),
                        },
                    )
                }
                Err(e) => TypedToolResult::error(t::switch_error(&ctx.resources.lang, e)),
            }
        } else {
            // 恢复上一上下文
            match ctx.workspace_control().exit() {
                Ok(prev) => {
                    let path_base = ctx.workspace_read().current_path_base();
                    let workspace_root = ctx.workspace_read().current_workspace_root();
                    let branch = get_current_branch(&workspace_root);
                    let headline = if ctx.resources.lang == "zh" {
                        format!("已退出 worktree，恢复到：{}", prev.path_base.display())
                    } else {
                        format!("Exited worktree, restored to: {}", prev.path_base.display())
                    };
                    TypedToolResult::success(
                        headline.clone(),
                        ExitWorktreeResult {
                            branch: branch.clone(),
                            path_base: path_base.clone(),
                            workspace_root: workspace_root.clone(),
                            guidance: t::exit_guidance(&ctx.resources.lang, &prev.path_base),
                        },
                    )
                }
                Err(e) => TypedToolResult::error(t::exit_error(&ctx.resources.lang, e)),
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
        // 全 Option 字段：生成的 schema 不含 required 键（或为空数组）。
        assert!(schema
            .get("required")
            .and_then(|v| v.as_array())
            .is_none_or(|arr| arr.is_empty()));
    }

    #[test]
    fn test_exit_worktree_schema() {
        let tool = ExitWorktreeTool;
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        // 全 Option 字段：生成的 schema 不含 required 键（或为空数组）。
        assert!(schema
            .get("required")
            .and_then(|v| v.as_array())
            .is_none_or(|arr| arr.is_empty()));
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
    fn test_workspace_context_payload_includes_path_base_and_workspace_root() {
        let payload = workspace_context_payload(
            "已进入 worktree：/repo/.worktrees/feature",
            "feature",
            Path::new("/repo/.worktrees/feature/subdir"),
            Path::new("/repo/.worktrees/feature"),
            "zh",
        );

        assert_eq!(payload.branch, "feature");
        assert_eq!(
            payload.path_base,
            Path::new("/repo/.worktrees/feature/subdir")
        );
        assert_eq!(
            payload.workspace_root,
            Path::new("/repo/.worktrees/feature")
        );
        // guidance 区分 path_base/workspace_root 语义（#413）
        assert!(payload.guidance.contains("path_base"));
        assert!(payload.guidance.contains("workspace_root"));
    }

    #[test]
    fn test_workspace_context_payload_guidance_bilingual() {
        let zh = workspace_context_payload("headline", "b", Path::new("/p"), Path::new("/p"), "zh");
        let en = workspace_context_payload("headline", "b", Path::new("/p"), Path::new("/p"), "en");
        assert!(zh.guidance.contains("相对路径"));
        assert!(en.guidance.contains("relative paths"));
    }
}
