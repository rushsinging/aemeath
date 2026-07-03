//! AgentClient trait 实现 — 薄委托到各子模块。

use async_trait::async_trait;
use sdk::{
    AgentClient, ChangeSet, ChatRequest, ChatStream, ClipboardImageView, CostInfo, ModelSummary,
    ProjectContext, ReflectionOutputView, SdkError, SessionSnapshot, SessionSummary,
    TaskStatusView,
};

use super::accessors::AgentClientImpl;

#[async_trait]
impl AgentClient for AgentClientImpl {
    // chat
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, SdkError> {
        super::trait_chat::chat_impl(self, input).await
    }

    // session
    async fn sync_current_messages(&self, messages: Vec<sdk::ChatMessage>) -> Result<(), SdkError> {
        super::trait_session::sync_current_messages_impl(self, messages).await
    }
    async fn save_current_session(&self) -> Result<(), SdkError> {
        super::trait_session::save_current_session_impl(self).await
    }
    async fn load_session(&self, id: &str) -> Result<SessionSnapshot, SdkError> {
        super::trait_session::load_session_impl(self, id).await
    }
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SdkError> {
        super::trait_session::list_sessions_impl(self).await
    }
    async fn delete_session(&self, id: &str) -> Result<(), SdkError> {
        super::trait_session::delete_session_impl(self, id).await
    }

    // command
    async fn estimate_context(
        &self,
        messages: &[sdk::ChatMessage],
        system_prompt: &str,
    ) -> Result<sdk::ContextEstimate, SdkError> {
        super::trait_compact::estimate_context_impl(self, messages, system_prompt).await
    }
    async fn switch_model(
        &self,
        params: sdk::ModelSwitchParams,
    ) -> Result<sdk::ModelSwitchResult, SdkError> {
        super::trait_model::switch_model_impl(self, params).await
    }
    async fn set_thinking(&self, desired: Option<bool>) -> Result<bool, SdkError> {
        super::trait_model::set_thinking_impl(self, desired).await
    }
    async fn compact_messages(
        &self,
        messages: Vec<sdk::ChatMessage>,
        system_prompt: &str,
        context_size: usize,
    ) -> Result<(Vec<sdk::ChatMessage>, bool), SdkError> {
        super::trait_compact::compact_messages_impl(self, messages, system_prompt, context_size)
            .await
    }
    async fn notify_hook(&self, message: &str, kind: &str) -> Result<(), SdkError> {
        super::trait_misc::notify_hook_impl(self, message, kind).await
    }
    async fn list_models(&self) -> Result<Vec<ModelSummary>, SdkError> {
        super::trait_model::list_models_impl(self).await
    }
    async fn compact(&self) -> Result<(), SdkError> {
        super::trait_compact::compact_impl(self).await
    }
    async fn read_clipboard_image(&self) -> Result<ClipboardImageView, SdkError> {
        super::trait_misc::read_clipboard_image_impl(self).await
    }
    async fn process_image_file(&self, path: String) -> Result<ClipboardImageView, SdkError> {
        super::trait_misc::process_image_file_impl(self, path).await
    }
    async fn run_reflection(
        &self,
        messages: Vec<sdk::ChatMessage>,
    ) -> Result<ReflectionOutputView, SdkError> {
        super::trait_reflection::run_reflection_impl(self, messages).await
    }
    async fn apply_reflection(&self, output: ReflectionOutputView) -> Result<String, SdkError> {
        super::trait_reflection::apply_reflection_impl(self, output).await
    }
    async fn list_reminders(&self) -> Result<Vec<sdk::ReminderView>, SdkError> {
        super::trait_memory::list_reminders_impl(self).await
    }
    async fn add_reminder(&self, content: &str) -> Result<String, SdkError> {
        super::trait_memory::add_reminder_impl(self, content).await
    }
    async fn complete_reminder(&self, id: &str) -> Result<(), SdkError> {
        super::trait_memory::complete_reminder_impl(self, id).await
    }
    async fn get_thinking(&self) -> Result<bool, SdkError> {
        super::trait_model::get_thinking_impl(self).await
    }

    // accessor methods (sync)
    fn session_snapshot(&self) -> SessionSnapshot {
        super::trait_accessor::session_snapshot_impl(self)
    }
    fn cost(&self) -> CostInfo {
        super::trait_accessor::cost_impl(self)
    }
    fn task_list(&self) -> Vec<sdk::TaskSummary> {
        super::trait_accessor::task_list_impl(self)
    }
    async fn task_status(&self) -> Result<TaskStatusView, SdkError> {
        super::trait_accessor::task_status_impl(self).await
    }
    fn project(&self) -> ProjectContext {
        super::trait_accessor::project_impl(self)
    }
    fn changes(&self) -> tokio::sync::watch::Receiver<ChangeSet> {
        super::trait_accessor::changes_impl(self)
    }
    fn cancel(&self) {
        super::trait_accessor::cancel_impl(self)
    }
    fn set_current_turn(&self, turn: usize) {
        super::trait_accessor::set_current_turn_impl(self, turn)
    }
    async fn restore_tasks(&self, snapshot: serde_json::Value) -> Result<(), SdkError> {
        super::trait_accessor::restore_tasks_impl(self, snapshot).await
    }
    async fn clear_tasks(&self) -> Result<(), SdkError> {
        super::trait_accessor::clear_tasks_impl(self).await
    }
}
