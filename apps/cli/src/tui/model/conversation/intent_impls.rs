//! 每个 intent struct 的 `impl ConversationUpdate`。
//!
//! 逻辑调用 ConversationModel 的现有 `pub(super)` 方法，再附带 spinner 维护。

use super::change::ConversationChange;
use super::intent::*;
use super::model::ConversationModel;
use super::processing_job::{ProcessingJob, ProcessingStatus};
use super::runtime_state::RuntimeState;
use super::stop_hook_notice::stop_hook_notice_content;
use super::task_status::TaskStatusSnapshot;
use super::tool_observe::ToolCallUpdateObservation;
use super::update::ConversationUpdate;

// ════════════════════════════════════════════════════════════════════
//  Conversation intent impls
// ════════════════════════════════════════════════════════════════════

impl ConversationUpdate for StartChat {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.start_chat(self.submission)
    }
}

impl ConversationUpdate for ResumeConversation {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        use super::history_parse::{
            collect_following_tool_results, normalize_tool_result_content,
            tool_result_content_to_string, tool_result_image_count, HistoryAssistantBlock,
            HistoryDisplayMessage,
        };
        use super::ids::{ChatId, ChatTurnId, ToolCallId};
        use super::tool_call::ToolCallStatus;

        let mut all_changes = Vec::new();
        const HISTORY_RESTORE_ERROR: &str =
            "无法恢复一条历史消息：消息格式不符合当前会话 schema，已跳过。";

        for (index, msg) in self.messages.iter().enumerate() {
            let subsequent = self.messages.get(index + 1);
            match HistoryDisplayMessage::parse(msg) {
                Ok(HistoryDisplayMessage::User { text }) => {
                    // 直接调 model.start_chat（不走 StartChat intent），避免 spinner 副作用。
                    all_changes.extend(model.start_chat(text));
                }
                Ok(HistoryDisplayMessage::StopHookNotice { .. }) => {
                    all_changes.extend(model.apply(AppendHookNotice {
                        content: stop_hook_notice_content(msg),
                    }));
                }
                Ok(HistoryDisplayMessage::ToolResults) => {}
                Ok(HistoryDisplayMessage::Assistant { blocks }) => {
                    let chat_id = model
                        .active_chat_id
                        .clone()
                        .unwrap_or_else(|| ChatId::from_legacy_or_new("history-chat"));
                    let turn_id = ChatTurnId::from_legacy_or_new("turn-1");
                    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
                    let tool_results = collect_following_tool_results(subsequent);
                    for (block_index, block) in blocks.into_iter().enumerate() {
                        match block {
                            HistoryAssistantBlock::Text(text) => {
                                all_changes.extend(model.apply(AssistantText {
                                    chat_id: chat_id.clone(),
                                    turn_id: turn_id.clone(),
                                    text,
                                }));
                                all_changes.extend(model.apply(CompleteBlock {
                                    chat_id: chat_id.clone(),
                                    turn_id: turn_id.clone(),
                                }));
                            }
                            HistoryAssistantBlock::Thinking(text) => {
                                all_changes.extend(model.apply(ThinkingText {
                                    chat_id: chat_id.clone(),
                                    turn_id: turn_id.clone(),
                                    text,
                                }));
                                all_changes.extend(model.apply(CompleteBlock {
                                    chat_id: chat_id.clone(),
                                    turn_id: turn_id.clone(),
                                }));
                            }
                            HistoryAssistantBlock::ToolUse { id, name, input } => {
                                let input_json = input.to_string();
                                let tool_call_id = ToolCallId::from_legacy_or_new(&id);
                                all_changes.extend(model.apply(ToolCallStart {
                                    chat_id: chat_id.clone(),
                                    turn_id: turn_id.clone(),
                                    id: tool_call_id.clone(),
                                    provider_id: None,
                                    name: name.clone(),
                                    index: block_index,
                                }));
                                all_changes.extend(model.apply(ToolCallUpdate {
                                    chat_id: chat_id.clone(),
                                    turn_id: turn_id.clone(),
                                    id: tool_call_id.clone(),
                                    provider_id: Some(id.clone()),
                                    name: name.clone(),
                                    index: block_index,
                                    arguments: Some(input_json),
                                    status: ToolCallStatus::Ready,
                                }));
                                if let Some(result) = tool_results.get(id.as_str()) {
                                    all_changes.extend(model.apply(ToolResult {
                                        chat_id: chat_id.clone(),
                                        turn_id: turn_id.clone(),
                                        id: tool_call_id.clone(),
                                        provider_id: id.clone(),
                                        tool_name: name,
                                        output: tool_result_content_to_string(result.content),
                                        content: normalize_tool_result_content(result.content),
                                        is_error: result.is_error,
                                        image_count: tool_result_image_count(result.content),
                                    }));
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    crate::tui::log_warn!("skip invalid history message during resume: {error}");
                    all_changes.extend(model.apply(AppendError {
                        text: HISTORY_RESTORE_ERROR.to_string(),
                    }));
                }
            }
        }
        all_changes
    }
}

impl ConversationUpdate for AppendUserMessage {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_user_message(self.text)
    }
}

impl ConversationUpdate for AssistantText {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_assistant_text(self.chat_id, self.turn_id, self.text)
    }
}

impl ConversationUpdate for ThinkingText {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_thinking_text(self.chat_id, self.turn_id, self.text)
    }
}

impl ConversationUpdate for CompleteBlock {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.complete_block(self.chat_id, self.turn_id)
    }
}

impl ConversationUpdate for ToolCallStart {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.start_tool_call(
            self.chat_id,
            self.turn_id,
            self.id,
            self.provider_id,
            self.name.clone(),
            self.index,
        )
    }
}

impl ConversationUpdate for ToolCallUpdate {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.update_tool_call(ToolCallUpdateObservation {
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

impl ConversationUpdate for ToolResult {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.complete_tool_call(
            self.chat_id,
            self.turn_id,
            self.id,
            self.provider_id,
            self.tool_name,
            self.output,
            self.content,
            self.is_error,
            self.image_count,
        )
    }
}

impl ConversationUpdate for AppendSystemMessage {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_system_message(self.text)
    }
}

impl ConversationUpdate for UpsertModelStreamPlaceholder {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.upsert_model_stream_placeholder(self.placeholder)
    }
}

impl ConversationUpdate for ClearModelStreamPlaceholder {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.clear_model_stream_placeholder()
    }
}

impl ConversationUpdate for AppendHookNotice {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_hook_notice(self.content)
    }
}

impl ConversationUpdate for AppendError {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_error(self.text)
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
        model.record_agent_progress(self.chat_id, self.turn_id, self.tool_id, self.message)
    }
}

impl ConversationUpdate for UpdateAgentMeta {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.update_agent_meta(
            self.chat_id,
            self.turn_id,
            self.tool_id,
            self.role,
            self.model,
        )
    }
}

impl ConversationUpdate for ShowAskUserBatch {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.show_ask_user_batch(self.slots)
    }
}

impl ConversationUpdate for AnswerCurrentAskUser {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.answer_current_ask_user(self.answer)
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

impl ConversationUpdate for MoveAskUserChatCursor {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.move_ask_user_chat_cursor(self.delta)
    }
}

impl ConversationUpdate for MoveAskUserChatCursorEnd {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.move_ask_user_chat_cursor_end(self.to_end)
    }
}

impl ConversationUpdate for DeleteAskUserChatWord {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.delete_ask_user_chat_word()
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
        model.confirm_ask_user_batch()
    }
}

impl ConversationUpdate for DismissAskUserBatch {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.dismiss_ask_user_batch()
    }
}

impl ConversationUpdate for ShowInteraction {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.show_interaction(self.request)
    }
}

impl ConversationUpdate for UpdateInteractionDraft {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.update_interaction_draft(&self.request_id, self.action)
    }
}

impl ConversationUpdate for ConfirmInteraction {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.confirm_interaction(&self.request_id)
    }
}

impl ConversationUpdate for CancelInteraction {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.cancel_interaction(&self.request_id)
    }
}

impl ConversationUpdate for InteractionReplyAccepted {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.accept_interaction(&self.request_id)
    }
}

impl ConversationUpdate for InteractionCancelAccepted {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.accept_interaction(&self.request_id)
    }
}

impl ConversationUpdate for InteractionReplyRejected {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.reject_interaction_reply(&self.request_id, self.failure)
    }
}

impl ConversationUpdate for InteractionCancelRejected {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.reject_interaction_cancel(&self.request_id, self.failure)
    }
}

impl ConversationUpdate for CompleteChat {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.complete_chat(self.chat_id, self.turn_id)
    }
}

// ════════════════════════════════════════════════════════════════════
//  Runtime intent impls（逻辑从 RuntimeModel::apply 搬入，
//  操作 ConversationModel 字段，返回 ConversationChange）
// ════════════════════════════════════════════════════════════════════

impl ConversationUpdate for RecordUsage {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.usage.input_tokens += self.input_tokens;
        model.runtime.usage.output_tokens += self.output_tokens;
        model.runtime.usage.last_input_tokens = self.last_input_tokens;
        model.runtime.usage.api_calls += 1;
        model.runtime.usage.cost_usd += self.cost_usd;
        vec![ConversationChange::UsageChanged {
            input_tokens: model.runtime.usage.input_tokens,
            output_tokens: model.runtime.usage.output_tokens,
            cost_usd: model.runtime.usage.cost_usd,
        }]
    }
}

impl ConversationUpdate for UpdateLastInputTokens {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.usage.last_input_tokens = self.0;
        vec![ConversationChange::UsageChanged {
            input_tokens: model.runtime.usage.input_tokens,
            output_tokens: model.runtime.usage.output_tokens,
            cost_usd: model.runtime.usage.cost_usd,
        }]
    }
}

impl ConversationUpdate for RecordLiveTps {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.live_tps = Some(self.tps);
        vec![ConversationChange::LiveTpsChanged { tps: self.tps }]
    }
}

impl ConversationUpdate for UpdateTaskStatus {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.task_status = TaskStatusSnapshot {
            total: self.total,
            completed: self.completed,
            in_progress: self.in_progress,
            lines: std::mem::take(&mut model.runtime.task_status.lines),
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
        model.runtime.processing_jobs.push(ProcessingJob {
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
            .runtime
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
        model.runtime.task_status.lines = self.0;
        vec![ConversationChange::TaskLinesChanged]
    }
}

impl ConversationUpdate for SetStatusNotice {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.status_notice = self.0;
        model.runtime.transient_notice_expiry = None;
        vec![ConversationChange::StatusNoticeChanged]
    }
}

impl ConversationUpdate for SetTransientStatusNotice {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.status_notice = self.notice;
        model.runtime.transient_notice_expiry = Some(self.expires_at);
        vec![ConversationChange::StatusNoticeChanged]
    }
}

impl ConversationUpdate for SetGraphPhase {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.graph_phase = self.0.clone();
        // 非 transient 时同步更新 status_notice
        if model.runtime.transient_notice_expiry.is_none() {
            model.runtime.status_notice = RuntimeState::notice_from_phase(self.0.as_deref());
        }
        vec![ConversationChange::GraphPhaseChanged]
    }
}

impl ConversationUpdate for SetCompactProgress {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model
            .runtime
            .set_compact_progress(self.stage, self.current, self.total);
        // 进度条嵌入 spinner 行（output 区），单独归类为 output_dirty 而非 status_dirty；
        // 见 `ConversationChange::CompactProgressChanged`（#540）。
        vec![ConversationChange::CompactProgressChanged]
    }
}

impl ConversationUpdate for SetSpinnerPhase {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.set_spinner_phase(self.phase);
        vec![ConversationChange::SpinnerPhaseChanged]
    }
}

impl ConversationUpdate for StopSpinner {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.stop_spinner();
        vec![ConversationChange::SpinnerStopped]
    }
}

impl ConversationUpdate for SyncQueuedSubmissions {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.sync_queued_submissions(self.queued)
    }
}

impl ConversationUpdate for ClearCompactRuntime {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.clear_compact_runtime()
    }
}

impl ConversationUpdate for RunStarted {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        if model.start_agent_run(self.run_id.clone()) {
            vec![ConversationChange::AgentRunChanged {
                run_id: self.run_id,
                phase: super::interaction::AgentRunPhase::Running,
            }]
        } else {
            Vec::new()
        }
    }
}

macro_rules! impl_run_transition {
    ($intent:ident, $phase:expr) => {
        impl ConversationUpdate for $intent {
            fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
                if model.transition_agent_run(&self.run_id, $phase) {
                    vec![ConversationChange::AgentRunChanged {
                        run_id: self.run_id,
                        phase: $phase,
                    }]
                } else {
                    Vec::new()
                }
            }
        }
    };
}

impl_run_transition!(
    RunAwaitingUser,
    super::interaction::AgentRunPhase::AwaitingUser
);
impl_run_transition!(RunResumed, super::interaction::AgentRunPhase::Running);
impl_run_transition!(RunCancelling, super::interaction::AgentRunPhase::Cancelling);
impl_run_transition!(RunCancelled, super::interaction::AgentRunPhase::Cancelled);
impl_run_transition!(RunCompleted, super::interaction::AgentRunPhase::Completed);
impl_run_transition!(RunFailed, super::interaction::AgentRunPhase::Failed);

impl ConversationUpdate for RunStepStarted {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        if model.start_agent_run_step(&self.run_id, self.step_id.clone(), self.tool_reference) {
            vec![ConversationChange::AgentRunStepChanged {
                run_id: self.run_id,
                step_id: self.step_id,
                phase: super::interaction::AgentRunStepPhase::Running,
            }]
        } else {
            Vec::new()
        }
    }
}

impl ConversationUpdate for RunStepCompleted {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        if model.complete_agent_run_step(&self.run_id, &self.step_id) {
            vec![ConversationChange::AgentRunStepChanged {
                run_id: self.run_id,
                step_id: self.step_id,
                phase: super::interaction::AgentRunStepPhase::Completed,
            }]
        } else {
            Vec::new()
        }
    }
}

// ════════════════════════════════════════════════════════════════════
//  ConversationIntent enum 的 ConversationUpdate 转发
// ════════════════════════════════════════════════════════════════════
impl ConversationUpdate for ConversationIntent {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        match self {
            Self::StartChat(s) => s.update(model),
            Self::ResumeConversation(s) => s.update(model),
            Self::AppendUserMessage(s) => s.update(model),
            Self::AssistantText(s) => s.update(model),
            Self::ThinkingText(s) => s.update(model),
            Self::CompleteBlock(s) => s.update(model),
            Self::ToolCallStart(s) => s.update(model),
            Self::ToolCallUpdate(s) => s.update(model),
            Self::ToolResult(s) => s.update(model),
            Self::AppendSystemMessage(s) => s.update(model),
            Self::UpsertModelStreamPlaceholder(s) => s.update(model),
            Self::ClearModelStreamPlaceholder(s) => s.update(model),
            Self::AppendHookNotice(s) => s.update(model),
            Self::AppendError(s) => s.update(model),
            Self::QueueSubmission(s) => s.update(model),
            Self::ClearQueuedSubmissionById(s) => s.update(model),
            Self::ClearAllQueuedSubmissions(s) => s.update(model),
            Self::RecordAgentProgress(s) => s.update(model),
            Self::UpdateAgentMeta(s) => s.update(model),
            Self::ShowAskUserBatch(s) => s.update(model),
            Self::AnswerCurrentAskUser(s) => s.update(model),
            Self::SetAskUserCursor(s) => s.update(model),
            Self::ToggleAskUserSelected(s) => s.update(model),
            Self::SetAskUserChatInput(s) => s.update(model),
            Self::AppendAskUserChatChar(s) => s.update(model),
            Self::DeleteAskUserChatChar(s) => s.update(model),
            Self::MoveAskUserChatCursor(s) => s.update(model),
            Self::MoveAskUserChatCursorEnd(s) => s.update(model),
            Self::DeleteAskUserChatWord(s) => s.update(model),
            Self::NavigateAskUserTo(s) => s.update(model),
            Self::SetAskUserConfirmCursor(s) => s.update(model),
            Self::ConfirmAskUserBatch(s) => s.update(model),
            Self::DismissAskUserBatch(s) => s.update(model),
            Self::ShowInteraction(s) => s.update(model),
            Self::UpdateInteractionDraft(s) => s.update(model),
            Self::ConfirmInteraction(s) => s.update(model),
            Self::CancelInteraction(s) => s.update(model),
            Self::InteractionReplyAccepted(s) => s.update(model),
            Self::InteractionCancelAccepted(s) => s.update(model),
            Self::InteractionReplyRejected(s) => s.update(model),
            Self::InteractionCancelRejected(s) => s.update(model),
            Self::RunStarted(s) => s.update(model),
            Self::RunAwaitingUser(s) => s.update(model),
            Self::RunResumed(s) => s.update(model),
            Self::RunCancelling(s) => s.update(model),
            Self::RunCancelled(s) => s.update(model),
            Self::RunCompleted(s) => s.update(model),
            Self::RunFailed(s) => s.update(model),
            Self::RunStepStarted(s) => s.update(model),
            Self::RunStepCompleted(s) => s.update(model),
            Self::CompleteChat(s) => s.update(model),
            Self::RecordUsage(s) => s.update(model),
            Self::UpdateLastInputTokens(s) => s.update(model),
            Self::RecordLiveTps(s) => s.update(model),
            Self::UpdateTaskStatus(s) => s.update(model),
            Self::StartProcessingJob(s) => s.update(model),
            Self::FinishProcessingJob(s) => s.update(model),
            Self::UpdateTaskLines(s) => s.update(model),
            Self::SetStatusNotice(s) => s.update(model),
            Self::SetTransientStatusNotice(s) => s.update(model),
            Self::SetGraphPhase(s) => s.update(model),
            Self::SetCompactProgress(s) => s.update(model),
            Self::SetSpinnerPhase(s) => s.update(model),
            Self::StopSpinner(s) => s.update(model),
            Self::SyncQueuedSubmissions(s) => s.update(model),
            Self::ClearCompactRuntime(s) => s.update(model),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::block::HookNoticeKind;
    use crate::tui::model::output_timeline::OutputTimelineItem;

    #[test]
    fn resume_projects_stop_hook_feedback_as_hook_notice() {
        let mut model = ConversationModel::default();
        let mut message = sdk::ChatMessage::system_generated_user_text(
            "<system-reminder>blocked by hook</system-reminder>",
        );
        message.metadata = Some(sdk::ChatMessageMetadata {
            source: sdk::ChatMessageSource::StopHook,
            stop_hook: Some(sdk::StopHookFeedbackView {
                summary: "blocked by hook".to_string(),
                command: "check-agent-stop.sh".to_string(),
                exit_code: Some(2),
                reason: "exit code 2".to_string(),
                stdout_preview: String::new(),
                stderr_preview: "blocked".to_string(),
                stdout_truncated: false,
                stderr_truncated: false,
                output_file: None,
            }),
        });

        ResumeConversation {
            messages: vec![message],
        }
        .update(&mut model);

        assert!(matches!(
            model.timeline.items().last(),
            Some(OutputTimelineItem::HookNotice { content, .. })
                if content.kind == HookNoticeKind::Blocked
                    && content.title == "Hook blocked: Stop"
                    && content.body == "blocked by hook"
                    && content.details.as_deref().is_some_and(|details|
                        details.contains("Command: check-agent-stop.sh")
                            && details.contains("Exit code: 2")
                    )
        ));
        assert!(model.timeline.items().iter().all(|item| {
            !matches!(item, OutputTimelineItem::UserMessage { text, .. } if text == "<system-reminder>blocked by hook</system-reminder>")
        }));
    }

    #[test]
    fn resume_interleaves_stop_hook_notice_without_user_message_projection() {
        let user = sdk::ChatMessage::user_text("user question");
        let mut stop_hook = sdk::ChatMessage::system_generated_user_text(
            "<system-reminder>blocked by hook</system-reminder>",
        );
        stop_hook.metadata = Some(sdk::ChatMessageMetadata {
            source: sdk::ChatMessageSource::StopHook,
            stop_hook: None,
        });
        let assistant = sdk::ChatMessage::assistant_text("assistant reply");
        let mut model = ConversationModel::default();

        ResumeConversation {
            messages: vec![user, stop_hook, assistant],
        }
        .update(&mut model);

        assert!(matches!(
            model.timeline.items().get(1),
            Some(OutputTimelineItem::HookNotice { content, .. })
                if content.kind == HookNoticeKind::Blocked && content.body == "blocked by hook"
        ));
        assert!(model.timeline.items().iter().all(|item| {
            !matches!(item, OutputTimelineItem::UserMessage { text, .. } if text.contains("blocked by hook"))
        }));
    }
}
