use crate::hook::data::*;
use crate::hook::result::HookResult;
use crate::hook::runner::HookRunner;
use share::config::hooks::HookEvent;

impl HookRunner {
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
