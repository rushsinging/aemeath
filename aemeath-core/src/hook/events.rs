//! Hook 运行器 — 事件便捷方法
//!
//! 每个便捷方法负责将参数包装为对应的 HookData 并调用 run_hooks 或 run_hooks_with_json。

use crate::config::hooks::{HookEntry, HookEvent};
use crate::hook::data::*;
use crate::hook::result::{HookJsonOutput, HookResult};
use crate::hook::runner::HookRunner;

impl HookRunner {
    /// 便捷方法：运行 PreToolUse hooks，返回是否应阻止
    pub async fn pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: serde_json::Value,
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
        )
        .await
    }

    /// 便捷方法：运行 UserPrompt hooks，返回是否应拒绝
    pub async fn user_prompt(&self, prompt: &str) -> (bool, Vec<HookResult>) {
        self.run_blocking_hooks(
            HookEvent::UserPromptSubmit,
            None,
            HookData::Prompt(PromptHookData {
                prompt: prompt.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 Stop hooks
    pub async fn on_stop(&self, turns: usize) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::Stop,
            None,
            HookData::Stop(StopHookData { turns }),
        )
        .await
    }

    /// 便捷方法：运行 SessionStart hooks，返回 JSON 输出
    pub async fn on_session_start(&self) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SessionStart,
            None,
            HookData::Session(SessionHookData {}),
        )
        .await
    }

    /// 便捷方法：运行 SessionEnd hooks，返回 JSON 输出
    pub async fn on_session_end(&self) -> Vec<(HookEntry, HookResult, Option<HookJsonOutput>)> {
        self.run_hooks_with_json(
            HookEvent::SessionEnd,
            None,
            HookData::Session(SessionHookData {}),
        )
        .await
    }

    /// 便捷方法：运行 PreCompact hooks，返回是否应阻止
    pub async fn pre_compact(
        &self,
        turns: usize,
        messages_count: usize,
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
        )
        .await
    }

    /// 便捷方法：运行 PostCompact hooks
    pub async fn post_compact(
        &self,
        turns: usize,
        messages_before: usize,
        messages_after: usize,
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
        )
        .await
    }

    /// 便捷方法：运行 SubagentStart hooks，返回 JSON 输出
    pub async fn on_subagent_start(
        &self,
        prompt: &str,
        system: &str,
        model_spec: Option<&str>,
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
        )
        .await
    }

    /// 便捷方法：运行 TaskCreated hooks，返回 JSON 输出
    pub async fn on_task_created(
        &self,
        tool_input: serde_json::Value,
        tool_output: &str,
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
        )
        .await
    }

    /// 便捷方法：运行 TaskCompleted hooks，返回 JSON 输出
    pub async fn on_task_completed(
        &self,
        tool_input: serde_json::Value,
        tool_output: &str,
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
        )
        .await
    }

    // ========== P2 便捷方法 ==========

    /// 便捷方法：运行 PermissionRequest hooks，返回是否应阻止
    pub async fn on_permission_request(
        &self,
        tool_name: &str,
        permission_rule: &str,
    ) -> (bool, Vec<HookResult>) {
        self.run_blocking_hooks(
            HookEvent::PermissionRequest,
            None,
            HookData::Permission(PermissionHookData {
                tool_name: tool_name.to_string(),
                permission_rule: permission_rule.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 PermissionDenied hooks
    pub async fn on_permission_denied(
        &self,
        tool_name: &str,
        permission_rule: &str,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::PermissionDenied,
            None,
            HookData::Permission(PermissionHookData {
                tool_name: tool_name.to_string(),
                permission_rule: permission_rule.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 Notification hooks
    pub async fn on_notification(
        &self,
        notification_text: &str,
        notification_type: &str,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::Notification,
            None,
            HookData::Notification(NotificationHookData {
                notification_text: notification_text.to_string(),
                notification_type: notification_type.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 InstructionsLoaded hooks
    pub async fn on_instructions_loaded(
        &self,
        file_path: &str,
        instruction_type: &str,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::InstructionsLoaded,
            None,
            HookData::InstructionsLoaded(InstructionsLoadedHookData {
                file_path: file_path.to_string(),
                instruction_type: instruction_type.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 ConfigChange hooks
    pub async fn on_config_change(
        &self,
        config_file: &str,
        changed_field: Option<&str>,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::ConfigChange,
            None,
            HookData::ConfigChange(ConfigChangeHookData {
                config_file: config_file.to_string(),
                changed_field: changed_field.map(String::from),
            }),
        )
        .await
    }

    /// 便捷方法：运行 Elicitation hooks，返回是否应阻止
    pub async fn on_elicitation(
        &self,
        server_name: &str,
        elicitation_text: &str,
    ) -> (bool, Vec<HookResult>) {
        self.run_blocking_hooks(
            HookEvent::Elicitation,
            None,
            HookData::Elicitation(ElicitationHookData {
                server_name: server_name.to_string(),
                elicitation_text: Some(elicitation_text.to_string()),
                user_response: None,
            }),
        )
        .await
    }

    /// 便捷方法：运行 ElicitationResult hooks
    pub async fn on_elicitation_result(
        &self,
        server_name: &str,
        user_response: &str,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::ElicitationResult,
            None,
            HookData::Elicitation(ElicitationHookData {
                server_name: server_name.to_string(),
                elicitation_text: None,
                user_response: Some(user_response.to_string()),
            }),
        )
        .await
    }

    // ========== P3 便捷方法 ==========

    /// 便捷方法：运行 UserPromptExpansion hooks，返回是否应拒绝
    pub async fn on_user_prompt_expansion(
        &self,
        original_input: &str,
        expanded_input: &str,
    ) -> (bool, Vec<HookResult>) {
        self.run_blocking_hooks(
            HookEvent::UserPromptExpansion,
            None,
            HookData::UserPromptExpansion(UserPromptExpansionHookData {
                original_input: original_input.to_string(),
                expanded_input: expanded_input.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 CwdChanged hooks
    pub async fn on_cwd_changed(&self, old_cwd: &str, new_cwd: &str) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::CwdChanged,
            None,
            HookData::CwdChanged(CwdChangedHookData {
                old_cwd: old_cwd.to_string(),
                new_cwd: new_cwd.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 FileChanged hooks
    pub async fn on_file_changed(&self, file_path: &str, change_type: &str) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::FileChanged,
            None,
            HookData::FileChanged(FileChangedHookData {
                file_path: file_path.to_string(),
                change_type: change_type.to_string(),
            }),
        )
        .await
    }

    /// 便捷方法：运行 TeammateIdle hooks
    pub async fn on_teammate_idle(
        &self,
        teammate_name: &str,
        idle_reason: Option<&str>,
    ) -> Vec<HookResult> {
        self.run_hooks(
            HookEvent::TeammateIdle,
            None,
            HookData::TeammateIdle(TeammateIdleHookData {
                teammate_name: teammate_name.to_string(),
                idle_reason: idle_reason.map(String::from),
            }),
        )
        .await
    }
}
