//! Hook 运行器 — 事件便捷方法
//!
//! 每个便捷方法负责将参数包装为对应的 HookData 并调用 run_hooks 或 run_hooks_with_json。

use crate::business::hook::data::*;
use crate::business::hook::result::{HookJsonOutput, HookResult};
use crate::business::hook::runner::HookRunner;
use share::config::hooks::{HookEntry, HookEvent};
use std::path::Path;

impl HookRunner {
    /// 便捷方法：运行 PreToolUse hooks，返回是否应阻止
    pub async fn pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: serde_json::Value,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> (bool, Vec<HookResult>) {
        self.run_blocking_hooks(
            HookEvent::PreToolUse,
            Some(tool_name),
            HookData::Tool(ToolHookData {
                tool_name: tool_name.to_string(),
                tool_input,
                tool_output: None,
                is_error: None,
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 PostToolUse hooks
    pub async fn post_tool_use(
        &self,
        tool_name: &str,
        tool_input: serde_json::Value,
        tool_output: &str,
        is_error: bool,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::PostToolUse,
            Some(tool_name),
            HookData::Tool(ToolHookData {
                tool_name: tool_name.to_string(),
                tool_input,
                tool_output: Some(tool_output.to_string()),
                is_error: Some(is_error),
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 UserPrompt hooks，返回是否应拒绝
    pub async fn user_prompt(
        &self,
        prompt: &str,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> (bool, Vec<HookResult>) {
        self.run_blocking_hooks(
            HookEvent::UserPromptSubmit,
            None,
            HookData::Prompt(PromptHookData {
                prompt: prompt.to_string(),
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 Stop hooks
    pub async fn on_stop(
        &self,
        turns: usize,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::Stop,
            None,
            HookData::Stop(StopHookData { turns }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 SessionStart hooks，返回 JSON 输出
    pub async fn on_session_start(
        &self,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SessionStart,
            None,
            HookData::Session(SessionHookData {}),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 SessionEnd hooks，返回 JSON 输出
    pub async fn on_session_end(
        &self,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SessionEnd,
            None,
            HookData::Session(SessionHookData {}),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 PreCompact hooks，返回是否应阻止
    pub async fn pre_compact(
        &self,
        turns: usize,
        messages_count: usize,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> (bool, Vec<HookResult>) {
        self.run_blocking_hooks(
            HookEvent::PreCompact,
            None,
            HookData::Compact(CompactHookData {
                turns,
                messages_before: messages_count,
                messages_after: None,
                was_compacted: false,
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 PostCompact hooks
    pub async fn post_compact(
        &self,
        turns: usize,
        messages_before: usize,
        messages_after: usize,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::PostCompact,
            None,
            HookData::Compact(CompactHookData {
                turns,
                messages_before,
                messages_after: Some(messages_after),
                was_compacted: true,
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 SubagentStart hooks，返回 JSON 输出
    pub async fn on_subagent_start(
        &self,
        prompt: &str,
        system: &str,
        model_spec: Option<&str>,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SubagentStart,
            None,
            HookData::Subagent(SubagentHookData {
                prompt: prompt.to_string(),
                system: system.to_string(),
                model_spec: model_spec.map(String::from),
                result: None,
                turns: None,
                is_error: None,
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 SubagentStop hooks，返回 JSON 输出
    pub async fn on_subagent_stop(
        &self,
        prompt: &str,
        system: &str,
        model_spec: Option<&str>,
        result: &str,
        turns: usize,
        is_error: bool,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SubagentStop,
            None,
            HookData::Subagent(SubagentHookData {
                prompt: prompt.to_string(),
                system: system.to_string(),
                model_spec: model_spec.map(String::from),
                result: Some(result.to_string()),
                turns: Some(turns),
                is_error: Some(is_error),
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 TaskCreated hooks，返回 JSON 输出
    pub async fn on_task_created(
        &self,
        tool_input: serde_json::Value,
        tool_output: &str,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::TaskCreated,
            None,
            HookData::Tool(ToolHookData {
                tool_name: "TaskCreate".to_string(),
                tool_input,
                tool_output: Some(tool_output.to_string()),
                is_error: Some(false),
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }

    /// 便捷方法：运行 TaskCompleted hooks，返回 JSON 输出
    pub async fn on_task_completed(
        &self,
        tool_input: serde_json::Value,
        tool_output: &str,
        workspace_root: &Path,
        in_worktree: bool,
    ) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::TaskCompleted,
            None,
            HookData::Tool(ToolHookData {
                tool_name: "TaskUpdate".to_string(),
                tool_input,
                tool_output: Some(tool_output.to_string()),
                is_error: Some(false),
            }),
            workspace_root,
            in_worktree,
        )
        .await
    }
}

mod extra;
