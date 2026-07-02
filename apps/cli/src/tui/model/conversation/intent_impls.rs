//! 每个 intent struct 的 `impl ConversationUpdate`。
//!
//! 逻辑调用 ConversationModel 的现有 `pub(super)` 方法，再附带 spinner 维护。

use super::change::ConversationChange;
use super::compact_progress::CompactProgressModel;
use super::intent::*;
use super::model::ConversationModel;
use super::processing_job::{ProcessingJob, ProcessingStatus};
use super::spinner::SpinnerPhase;
use super::task_status::TaskStatusSnapshot;
use super::tool_observe::ToolCallUpdateObservation;
use super::update::ConversationUpdate;

// ════════════════════════════════════════════════════════════════════
//  Conversation intent impls
// ════════════════════════════════════════════════════════════════════

impl ConversationUpdate for StartChat {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.start_chat(self.submission);
        // #536: spinner 可见性跟随 chat 生命周期，StartChat 时立即激活。
        model.spinner.chat_active = true;
        model.spinner.phase = Some(SpinnerPhase::Thinking);
        changes
    }
}

impl ConversationUpdate for AppendUserMessage {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_user_message(self.text)
    }
}

impl ConversationUpdate for ObserveAssistantText {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.append_assistant_text(self.chat_id, self.turn_id, self.text);
        if !changes.is_empty() {
            model.spinner.phase = Some(SpinnerPhase::Generating);
        }
        changes
    }
}

impl ConversationUpdate for ObserveThinkingText {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.append_thinking_text(self.chat_id, self.turn_id, self.text);
        if !changes.is_empty() {
            model.spinner.phase = Some(SpinnerPhase::Thinking);
        }
        changes
    }
}

impl ConversationUpdate for CompleteBlock {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.complete_block(self.chat_id, self.turn_id)
    }
}

impl ConversationUpdate for ObserveToolCallStart {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.observe_tool_call_start(
            self.chat_id,
            self.turn_id,
            self.id,
            self.provider_id,
            self.name.clone(),
            self.index,
        );
        if !changes.is_empty() {
            model.spinner.running_tool_count += 1;
            model.spinner.phase = Some(SpinnerPhase::CallingTool(self.name));
        }
        changes
    }
}

impl ConversationUpdate for ObserveToolCallUpdate {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.observe_tool_call_update(ToolCallUpdateObservation {
            chat_id: self.chat_id,
            turn_id: self.turn_id,
            id: self.id,
            provider_id: self.provider_id,
            name: self.name,
            index: self.index,
            arguments: self.arguments,
            status: self.status,
        })
    }
}

impl ConversationUpdate for ObserveToolResult {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.observe_tool_result(
            self.chat_id,
            self.turn_id,
            self.id,
            self.provider_id,
            self.tool_name,
            self.output,
            self.content,
            self.is_error,
            self.image_count,
        );
        if !changes.is_empty() {
            model.spinner.running_tool_count = model.spinner.running_tool_count.saturating_sub(1);
            if model.spinner.running_tool_count == 0 {
                model.spinner.phase = Some(SpinnerPhase::Thinking);
            } else {
                model.spinner.phase = Some(SpinnerPhase::CallingTools {
                    remaining: model.spinner.running_tool_count,
                });
            }
        }
        changes
    }
}

impl ConversationUpdate for AppendSystemMessage {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_system_message(self.text)
    }
}

impl ConversationUpdate for AppendHookNotice {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_hook_notice(self.content)
    }
}

impl ConversationUpdate for AppendError {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.append_error(self.text);
        if !changes.is_empty() {
            model.spinner.chat_active = false;
            model.spinner.running_tool_count = 0;
            model.spinner.phase = None;
        }
        changes
    }
}
impl ConversationUpdate for QueueSubmission {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.queue_submission(self.input_id, self.text)
    }
}

impl ConversationUpdate for ClearQueuedSubmissionById {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.clear_queued_submission_by_id(&self.input_id)
    }
}

impl ConversationUpdate for ClearAllQueuedSubmissions {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.clear_all_queued_submissions()
    }
}

impl ConversationUpdate for RecordAgentProgress {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes =
            model.record_agent_progress(self.chat_id, self.turn_id, self.tool_id, self.message);
        if !changes.is_empty() {
            model.spinner.phase = Some(SpinnerPhase::AgentWorking);
        }
        changes
    }
}

impl ConversationUpdate for ShowAskUserBatch {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.show_ask_user_batch(self.slots);
        if !changes.is_empty() {
            // #536: AskUser 暂停 spinner（chat_active=false），用户回答后恢复。
            model.spinner.chat_active = false;
            model.spinner.phase = None;
        }
        changes
    }
}

impl ConversationUpdate for AnswerCurrentAskUser {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.answer_current_ask_user(self.answer);
        // #536: AskUser 应答后恢复 spinner，继续等待 LLM 回复。
        if !changes.is_empty() {
            model.spinner.chat_active = true;
            model.spinner.phase = Some(SpinnerPhase::Thinking);
        }
        changes
    }
}

impl ConversationUpdate for SetAskUserCursor {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.set_ask_user_cursor(self.cursor)
    }
}

impl ConversationUpdate for ToggleAskUserSelected {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.toggle_ask_user_selected(self.index)
    }
}

impl ConversationUpdate for SetAskUserChatInput {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.set_ask_user_chat_input(self.active)
    }
}

impl ConversationUpdate for AppendAskUserChatChar {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_ask_user_chat_char(self.ch)
    }
}

impl ConversationUpdate for DeleteAskUserChatChar {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.delete_ask_user_chat_char()
    }
}

impl ConversationUpdate for NavigateAskUserTo {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.navigate_ask_user_to(self.index)
    }
}

impl ConversationUpdate for SetAskUserConfirmCursor {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.set_ask_user_confirm_cursor(self.cursor)
    }
}

impl ConversationUpdate for ConfirmAskUserBatch {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.confirm_ask_user_batch();
        // #536: AskUser 确认后恢复 spinner，继续等待 LLM 回复。
        if !changes.is_empty() {
            model.spinner.chat_active = true;
            model.spinner.phase = Some(SpinnerPhase::Thinking);
        }
        changes
    }
}

impl ConversationUpdate for DismissAskUserBatch {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.dismiss_ask_user_batch();
        // #536: AskUser 取消后恢复 spinner，继续等待 LLM 回复。
        if !changes.is_empty() {
            model.spinner.chat_active = true;
            model.spinner.phase = Some(SpinnerPhase::Thinking);
        }
        changes
    }
}

impl ConversationUpdate for CompleteChat {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        let changes = model.complete_chat(self.chat_id, self.turn_id);
        if !changes.is_empty() {
            model.spinner.chat_active = false;
            model.spinner.running_tool_count = 0;
            model.spinner.phase = None;
        }
        changes
    }
}

// ════════════════════════════════════════════════════════════════════
//  Runtime intent impls（逻辑从 RuntimeModel::apply 搬入，
//  操作 ConversationModel 字段，返回 ConversationChange）
// ════════════════════════════════════════════════════════════════════

impl ConversationUpdate for SetProviderModel {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.provider = self.provider.clone();
        model.model_id = self.model_id.clone();
        vec![ConversationChange::ProviderModelChanged {
            provider: self.provider,
            model_id: self.model_id,
        }]
    }
}

impl ConversationUpdate for UpdateWorkspace {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.workspace.cwd = Some(self.cwd.clone());
        model.workspace.worktree = self.worktree.clone();
        vec![ConversationChange::WorkspaceChanged {
            cwd: self.cwd,
            worktree: self.worktree,
        }]
    }
}

impl ConversationUpdate for WorkspaceSnapshotReceived {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.workspace.path_base = self.path_base.clone();
        model.workspace.workspace_root = self.workspace_root.clone();
        model.workspace.branch = self.branch.clone();
        model.workspace.kind = self.kind;
        vec![ConversationChange::WorkspaceSnapshotChanged {
            path_base: self.path_base,
            workspace_root: self.workspace_root,
            branch: self.branch,
            kind: self.kind,
        }]
    }
}

impl ConversationUpdate for RecordUsage {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.usage.input_tokens += self.input_tokens;
        model.usage.output_tokens += self.output_tokens;
        model.usage.last_input_tokens = self.last_input_tokens;
        model.usage.api_calls += 1;
        model.usage.cost_usd += self.cost_usd;
        vec![ConversationChange::UsageChanged {
            input_tokens: model.usage.input_tokens,
            output_tokens: model.usage.output_tokens,
            cost_usd: model.usage.cost_usd,
        }]
    }
}

impl ConversationUpdate for SetContextSize {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.usage.context_size = self.0;
        vec![ConversationChange::UsageChanged {
            input_tokens: model.usage.input_tokens,
            output_tokens: model.usage.output_tokens,
            cost_usd: model.usage.cost_usd,
        }]
    }
}

impl ConversationUpdate for UpdateLastInputTokens {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.usage.last_input_tokens = self.0;
        vec![ConversationChange::UsageChanged {
            input_tokens: model.usage.input_tokens,
            output_tokens: model.usage.output_tokens,
            cost_usd: model.usage.cost_usd,
        }]
    }
}

impl ConversationUpdate for RecordLiveTps {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.live_tps = Some(self.tps);
        vec![ConversationChange::LiveTpsChanged { tps: self.tps }]
    }
}

impl ConversationUpdate for UpdateTaskStatus {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.task_status = TaskStatusSnapshot {
            total: self.total,
            completed: self.completed,
            in_progress: self.in_progress,
            lines: std::mem::take(&mut model.task_status.lines),
        };
        vec![ConversationChange::TaskStatusChanged {
            total: self.total,
            completed: self.completed,
            in_progress: self.in_progress,
        }]
    }
}

impl ConversationUpdate for StartProcessingJob {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.processing_jobs.push(ProcessingJob {
            id: self.id.clone(),
            chat_id: self.chat_id,
            status: ProcessingStatus::Running,
        });
        vec![ConversationChange::ProcessingJobChanged { id: self.id }]
    }
}

impl ConversationUpdate for FinishProcessingJob {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        if let Some(job) = model
            .processing_jobs
            .iter_mut()
            .find(|job| job.id == self.id)
        {
            job.status = if self.success {
                ProcessingStatus::Finished
            } else {
                ProcessingStatus::Failed
            };
        }
        vec![ConversationChange::ProcessingJobChanged { id: self.id }]
    }
}

impl ConversationUpdate for UpdateTaskLines {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.task_status.lines = self.0;
        vec![ConversationChange::TaskLinesChanged]
    }
}

impl ConversationUpdate for SetStatusNotice {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.status_notice = self.0;
        model.transient_notice_expiry = None;
        vec![ConversationChange::StatusNoticeChanged]
    }
}

impl ConversationUpdate for SetTransientStatusNotice {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.status_notice = self.notice;
        model.transient_notice_expiry = Some(self.expires_at);
        vec![ConversationChange::StatusNoticeChanged]
    }
}

impl ConversationUpdate for SetThinking {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.thinking = self.0;
        vec![ConversationChange::ThinkingChanged]
    }
}

impl ConversationUpdate for SetGraphPhase {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.graph_phase = self.0.clone();
        // 非 transient 时同步更新 status_notice
        if model.transient_notice_expiry.is_none() {
            model.status_notice = ConversationModel::notice_from_phase(self.0.as_deref());
        }
        vec![ConversationChange::GraphPhaseChanged]
    }
}

impl ConversationUpdate for SetCompactProgress {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.compact_progress = Some(CompactProgressModel {
            stage: self.stage,
            current: self.current,
            total: self.total,
        });
        // 附带 spinner 维护：compact 期间 spinner 可见
        model.spinner.chat_active = true;
        model.spinner.phase = Some(SpinnerPhase::Compacting);
        vec![ConversationChange::SpinnerPhaseChanged]
    }
}

// ════════════════════════════════════════════════════════════════════
//  ConversationIntent enum 的 ConversationUpdate 转发
// ════════════════════════════════════════════════════════════════════

impl ConversationUpdate for ConversationIntent {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        match self {
            Self::StartChat(s) => s.update(model),
            Self::AppendUserMessage(s) => s.update(model),
            Self::ObserveAssistantText(s) => s.update(model),
            Self::ObserveThinkingText(s) => s.update(model),
            Self::CompleteBlock(s) => s.update(model),
            Self::ObserveToolCallStart(s) => s.update(model),
            Self::ObserveToolCallUpdate(s) => s.update(model),
            Self::ObserveToolResult(s) => s.update(model),
            Self::AppendSystemMessage(s) => s.update(model),
            Self::AppendHookNotice(s) => s.update(model),
            Self::AppendError(s) => s.update(model),
            Self::QueueSubmission(s) => s.update(model),
            Self::ClearQueuedSubmissionById(s) => s.update(model),
            Self::ClearAllQueuedSubmissions(s) => s.update(model),
            Self::RecordAgentProgress(s) => s.update(model),
            Self::ShowAskUserBatch(s) => s.update(model),
            Self::AnswerCurrentAskUser(s) => s.update(model),
            Self::SetAskUserCursor(s) => s.update(model),
            Self::ToggleAskUserSelected(s) => s.update(model),
            Self::SetAskUserChatInput(s) => s.update(model),
            Self::AppendAskUserChatChar(s) => s.update(model),
            Self::DeleteAskUserChatChar(s) => s.update(model),
            Self::NavigateAskUserTo(s) => s.update(model),
            Self::SetAskUserConfirmCursor(s) => s.update(model),
            Self::ConfirmAskUserBatch(s) => s.update(model),
            Self::DismissAskUserBatch(s) => s.update(model),
            Self::CompleteChat(s) => s.update(model),
            Self::SetProviderModel(s) => s.update(model),
            Self::UpdateWorkspace(s) => s.update(model),
            Self::WorkspaceSnapshotReceived(s) => s.update(model),
            Self::RecordUsage(s) => s.update(model),
            Self::SetContextSize(s) => s.update(model),
            Self::UpdateLastInputTokens(s) => s.update(model),
            Self::RecordLiveTps(s) => s.update(model),
            Self::UpdateTaskStatus(s) => s.update(model),
            Self::StartProcessingJob(s) => s.update(model),
            Self::FinishProcessingJob(s) => s.update(model),
            Self::UpdateTaskLines(s) => s.update(model),
            Self::SetStatusNotice(s) => s.update(model),
            Self::SetTransientStatusNotice(s) => s.update(model),
            Self::SetThinking(s) => s.update(model),
            Self::SetGraphPhase(s) => s.update(model),
            Self::SetCompactProgress(s) => s.update(model),
        }
    }
}
