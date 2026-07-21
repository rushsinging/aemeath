//! Enter/Exit Worktree tools
//!
//! These tools allow the agent to switch between git worktree directories
//! while maintaining a context stack for nested worktree navigation.

use crate::domain::types::enter_worktree::{EnterWorktreeInput, EnterWorktreeResult};
use crate::domain::types::exit_worktree::{ExitWorktreeInput, ExitWorktreeResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::i18n::tools::worktree as t;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

fn current_branch(ctx: &ToolExecutionContext) -> String {
    ctx.workspace_read()
        .current_branch()
        .ok()
        .flatten()
        .unwrap_or_else(|| "(unknown)".to_string())
}
use project::WorkspaceControl;
use std::sync::Arc;

/// Tool to enter a git worktree directory
pub struct EnterWorktreeTool {
    pub control: Arc<dyn WorkspaceControl>,
}

/// Tool to exit the current worktree and restore the previous context
pub struct ExitWorktreeTool {
    pub control: Arc<dyn WorkspaceControl>,
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
        use crate::domain::types::ToolSchema;
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
                return TypedToolResult::error(t::invalid_input_error(ctx.guidance().language(), e))
            }
        };

        let path = args
            .path
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);
        let display_target = path
            .as_ref()
            .map(|value| value.display().to_string())
            .unwrap_or_else(|| {
                args.branch
                    .clone()
                    .map(|branch| format!("branch {branch}"))
                    .unwrap_or_else(|| {
                        if ctx.guidance().language() == "zh" {
                            "未指定目标".to_string()
                        } else {
                            "(unspecified)".to_string()
                        }
                    })
            });

        match self.control.enter(path, args.branch.clone(), args.base) {
            Ok(_frame) => {
                let path_base = ctx.workspace_read().current_path_base();
                let workspace_root = ctx.workspace_read().current_workspace_root();
                let branch = current_branch(ctx);
                let headline = if ctx.guidance().language() == "zh" {
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
                        ctx.guidance().language(),
                    ),
                )
            }
            Err(e) => TypedToolResult::error(t::enter_error(ctx.guidance().language(), e)),
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
        use crate::domain::types::ToolSchema;
        ExitWorktreeInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
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
                return TypedToolResult::error(t::invalid_input_error(ctx.guidance().language(), e))
            }
        };

        if let Some(path) = args.path {
            // 直接切到指定路径：校验存在性 + 同源，不污染上下文栈（不留多余栈帧）。
            match self.control.switch_to(PathBuf::from(&path)) {
                Ok(()) => {
                    let path_base = ctx.workspace_read().current_path_base();
                    let workspace_root = ctx.workspace_read().current_workspace_root();
                    let branch = current_branch(ctx);
                    let headline = if ctx.guidance().language() == "zh" {
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
                            guidance: t::switch_guidance(ctx.guidance().language(), &path),
                        },
                    )
                }
                Err(e) => TypedToolResult::error(t::switch_error(ctx.guidance().language(), e)),
            }
        } else {
            // 恢复上一上下文
            match self.control.exit() {
                Ok(prev) => {
                    let path_base = ctx.workspace_read().current_path_base();
                    let workspace_root = ctx.workspace_read().current_workspace_root();
                    let branch = current_branch(ctx);
                    let headline = if ctx.guidance().language() == "zh" {
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
                            guidance: t::exit_guidance(ctx.guidance().language(), &prev.path_base),
                        },
                    )
                }
                Err(e) => TypedToolResult::error(t::exit_error(ctx.guidance().language(), e)),
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
#[path = "worktree_tests.rs"]
mod tests;
