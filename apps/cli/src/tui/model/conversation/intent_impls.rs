//! 每个 intent struct 的 `impl ConversationUpdate`。
//!
//! 逻辑调用 ConversationModel 的现有 `pub(super)` 方法，再附带 spinner 维护。

use super::change::ConversationChange;
use super::intent::*;
use super::model::ConversationModel;
use super::processing_job::{ProcessingJob, ProcessingStatus};
use super::runtime_state::RuntimeState;
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
            collect_following_tool_results, normalize_tool_result_content, restored_ask_answer,
            tool_result_display_text, tool_result_image_count, HistoryAssistantBlock,
            HistoryDisplayMessage,
        };
        use super::ids::{ChatId, ChatRunId, ToolCallId};
        use super::terminal::TerminalCause;
        use super::tool_call::ToolCallStatus;

        let mut all_changes = Vec::new();
        model.reset();
        const HISTORY_RESTORE_ERROR: &str =
            "无法恢复一条历史消息：消息格式不符合当前会话 schema，已跳过。";

        for step in &self.steps {
            let chat_id = ChatId::from_legacy_or_new(&step.run_id);
            let run_id = ChatRunId::from_legacy_or_new(&step.step_id);
            model.ensure_runtime_turn(chat_id.clone(), run_id.clone());
            for (index, msg) in step.messages.iter().enumerate() {
                let subsequent = step.messages.get(index + 1);
                match HistoryDisplayMessage::parse(msg) {
                    Ok(HistoryDisplayMessage::User { text }) => {
                        all_changes.extend(model.apply(AppendUserMessage { text }));
                    }
                    Ok(HistoryDisplayMessage::HookNotice { title, text, kind }) => {
                        all_changes.extend(model.apply(AppendHookNotice { title, text, kind }));
                    }
                    Ok(HistoryDisplayMessage::TypedJson { text }) => {
                        all_changes.extend(model.apply(AppendSystemMessage { text }));
                    }
                    Ok(HistoryDisplayMessage::ToolResults) => {}
                    Ok(HistoryDisplayMessage::Assistant { blocks }) => {
                        let tool_results = collect_following_tool_results(subsequent);
                        let mut restored_ask_slots = Vec::new();
                        for (block_index, block) in blocks.into_iter().enumerate() {
                            match block {
                                HistoryAssistantBlock::Text(text) => {
                                    all_changes.extend(model.apply(AssistantText {
                                        chat_id: chat_id.clone(),
                                        run_id: run_id.clone(),
                                        text,
                                    }));
                                    all_changes.extend(model.apply(CompleteBlock {
                                        chat_id: chat_id.clone(),
                                        run_id: run_id.clone(),
                                    }));
                                }
                                HistoryAssistantBlock::Thinking(text) => {
                                    all_changes.extend(model.apply(ThinkingText {
                                        chat_id: chat_id.clone(),
                                        run_id: run_id.clone(),
                                        text,
                                    }));
                                    all_changes.extend(model.apply(CompleteBlock {
                                        chat_id: chat_id.clone(),
                                        run_id: run_id.clone(),
                                    }));
                                }
                                HistoryAssistantBlock::ToolUse { id, name, input } => {
                                    let ask_question = (name == "AskUserQuestion")
                                        .then(|| {
                                            input.get("question").and_then(|value| value.as_str())
                                        })
                                        .flatten()
                                        .filter(|question| !question.trim().is_empty())
                                        .map(ToOwned::to_owned);
                                    let restored_answer = tool_results
                                        .get(id.as_str())
                                        .copied()
                                        .and_then(restored_ask_answer);
                                    let input_json = input.to_string();
                                    let tool_call_id = ToolCallId::from_legacy_or_new(&id);
                                    all_changes.extend(model.apply(ToolCallStart {
                                        chat_id: chat_id.clone(),
                                        run_id: run_id.clone(),
                                        id: tool_call_id.clone(),
                                        provider_id: None,
                                        name: name.clone(),
                                        index: block_index,
                                    }));
                                    all_changes.extend(model.apply(ToolCallUpdate {
                                        chat_id: chat_id.clone(),
                                        run_id: run_id.clone(),
                                        id: tool_call_id.clone(),
                                        provider_id: Some(id.clone()),
                                        name: name.clone(),
                                        index: block_index,
                                        arguments: Some(input_json),
                                        status: ToolCallStatus::Ready,
                                    }));
                                    if let (Some(question), Some(answer)) =
                                        (ask_question, restored_answer)
                                    {
                                        restored_ask_slots.push(super::block::AskUserSlot {
                                            id: id.clone(),
                                            question_seq: 0,
                                            question,
                                            options: Vec::new(),
                                            llm_option_count: 0,
                                            multi_select: false,
                                            default: None,
                                            answer: Some(answer),
                                        });
                                    }
                                    if let Some(result) = tool_results.get(id.as_str()) {
                                        all_changes.extend(model.apply(ToolResult {
                                            chat_id: chat_id.clone(),
                                            run_id: run_id.clone(),
                                            id: tool_call_id.clone(),
                                            provider_id: id.clone(),
                                            tool_name: name,
                                            output: tool_result_display_text(*result),
                                            content: normalize_tool_result_content(result.content),
                                            is_error: result.is_error,
                                            image_count: tool_result_image_count(result.content),
                                        }));
                                    } else {
                                        // #1384: ToolUse without a matching ToolResult means
                                        // the tool was cancelled (e.g. agent call interrupted).
                                        // Mark as Cancelled so it doesn't stay in Ready/Running.
                                        all_changes.extend(model.apply(ToolCallUpdate {
                                            chat_id: chat_id.clone(),
                                            run_id: run_id.clone(),
                                            id: tool_call_id.clone(),
                                            provider_id: Some(id.clone()),
                                            name: name.clone(),
                                            index: block_index,
                                            arguments: None,
                                            status: ToolCallStatus::Cancelled,
                                        }));
                                    }
                                }
                            }
                        }
                        if !restored_ask_slots.is_empty() {
                            all_changes
                                .extend(model.restore_answered_ask_user_batch(restored_ask_slots));
                        }
                    }
                    Err(super::history_parse::HistoryDisplayParseError::NonUserVisibleMessage) => {}
                    Err(error) => {
                        crate::tui::log_warn!(
                            "skip invalid history message during resume: {error}"
                        );
                        all_changes.extend(model.apply(AppendError {
                            text: HISTORY_RESTORE_ERROR.to_string(),
                        }));
                    }
                }
            }
            if let Some(finalize_cause) = step.finalize_cause {
                let cause = match finalize_cause {
                    crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::Completed => {
                        TerminalCause::Completed
                    }
                    crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep => {
                        TerminalCause::UserCancelled
                    }
                    crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::RunTerminated => {
                        TerminalCause::RunTerminated
                    }
                };
                all_changes.extend(model.apply(TerminalNotice {
                    cause,
                    duration: step.duration_ms.map(std::time::Duration::from_millis),
                }));
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
        model.append_assistant_text(self.chat_id, self.run_id, self.text)
    }
}

impl ConversationUpdate for ThinkingText {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_thinking_text(self.chat_id, self.run_id, self.text)
    }
}

impl ConversationUpdate for CompleteBlock {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.complete_block(self.chat_id, self.run_id)
    }
}

impl ConversationUpdate for ToolCallStart {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.start_tool_call(
            self.chat_id,
            self.run_id,
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
            run_id: self.run_id,
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
            self.run_id,
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

impl ConversationUpdate for TerminalNotice {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        super::terminal::terminal_notice(self.cause, self.duration)
            .map_or_else(Vec::new, |text| model.append_system_message(text))
    }
}

impl ConversationUpdate for PresentCancelledStep {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.present_cancelled_step(self.confirmed)
    }
}

impl ConversationUpdate for AppendHookNotice {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_hook_notice(self.title, self.text, self.kind)
    }
}

impl ConversationUpdate for AppendSystemMessage {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.append_system_message(self.text)
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

impl ConversationUpdate for RecordChildRunActivity {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.record_child_run_activity(self)
    }
}

impl ConversationUpdate for RecordAgentProgress {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.record_agent_progress(self.chat_id, self.run_id, self.tool_id, self.message)
    }
}

impl ConversationUpdate for RecordToolStreamingOutput {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.record_tool_streaming_output(self.chat_id, self.run_id, self.tool_id, self.text)
    }
}

impl ConversationUpdate for UpdateAgentMeta {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.update_agent_meta(
            self.chat_id,
            self.run_id,
            self.tool_id,
            self.role,
            self.model,
        )
    }
}

impl ConversationUpdate for ShowAskUserBatch {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.show_ask_user_batch(self.request_id, self.slots)
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
        model.accept_interaction_reply(&self.request_id)
    }
}

impl ConversationUpdate for InteractionCancelAccepted {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.accept_interaction_cancel(&self.request_id)
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
        model.complete_chat(self.chat_id, self.run_id)
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
            ..TaskStatusSnapshot::default()
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

impl ConversationUpdate for ReplaceTaskState {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        if model.runtime.task_status.replace(self.0) {
            vec![ConversationChange::TaskLinesChanged]
        } else {
            Vec::new()
        }
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

impl ConversationUpdate for ObserveActivityChange {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.observe_activity_change(self.activity)
    }
}

impl ConversationUpdate for ReplaceActivitySnapshot {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.replace_activity_snapshot(self.snapshot)
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
            Self::TerminalNotice(s) => s.update(model),
            Self::PresentCancelledStep(s) => s.update(model),
            Self::AppendHookNotice(s) => s.update(model),
            Self::AppendSystemMessage(s) => s.update(model),
            Self::AppendError(s) => s.update(model),
            Self::QueueSubmission(s) => s.update(model),
            Self::ClearQueuedSubmissionById(s) => s.update(model),
            Self::ClearAllQueuedSubmissions(s) => s.update(model),
            Self::RecordChildRunActivity(s) => s.update(model),
            Self::RecordAgentProgress(s) => s.update(model),
            Self::RecordToolStreamingOutput(s) => s.update(model),
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
            Self::ObserveActivityChange(s) => s.update(model),
            Self::ReplaceActivitySnapshot(s) => s.update(model),
            Self::CompleteChat(s) => s.update(model),
            Self::RecordUsage(s) => s.update(model),
            Self::UpdateLastInputTokens(s) => s.update(model),
            Self::RecordLiveTps(s) => s.update(model),
            Self::UpdateTaskStatus(s) => s.update(model),
            Self::StartProcessingJob(s) => s.update(model),
            Self::FinishProcessingJob(s) => s.update(model),
            Self::ReplaceTaskState(state) => state.update(model),
            Self::UpdateTaskLines(state) => state.update(model),
            Self::SetStatusNotice(s) => s.update(model),
            Self::SetTransientStatusNotice(s) => s.update(model),
            Self::SetGraphPhase(s) => s.update(model),
            Self::SetCompactProgress(s) => s.update(model),
            Self::SyncQueuedSubmissions(s) => s.update(model),
            Self::ClearCompactRuntime(s) => s.update(model),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock, TuiMessageSource};
    use crate::tui::model::conversation::tool_call::ToolCallStatus;
    use crate::tui::model::output_timeline::OutputTimelineItem;

    fn ask_tool_use(id: &str, question: &str) -> TuiContentBlock {
        TuiContentBlock::ToolUse {
            id: id.to_string(),
            name: "AskUserQuestion".to_string(),
            input: serde_json::json!({ "question": question }),
        }
    }

    fn ask_result(id: &str, answer: serde_json::Value) -> TuiChatMessage {
        TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: answer,
                is_error: false,
                text: None,
            }],
            input_id: None,
            source: TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
        }
    }

    #[test]
    fn resume_projects_completed_step_terminal_notice_when_duration_is_known() {
        let mut model = ConversationModel::default();

        ResumeConversation {
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "completed-run".into(),
                step_id: "completed-step".into(),
                messages: vec![TuiChatMessage::user_text("completed question")],
                finalize_cause: Some(
                    crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::Completed,
                ),
                duration_ms: Some(125_000),
            }],
        }
        .update(&mut model);

        assert_eq!(
            model
                .timeline
                .items()
                .iter()
                .filter(|item| matches!(
                    item,
                    OutputTimelineItem::System { text, .. }
                        if text.starts_with('✻') && text.ends_with("for 2m 5s")
                ))
                .count(),
            1
        );
        assert!(!model.timeline.items().iter().any(|item| matches!(
            item,
            OutputTimelineItem::System { text, .. }
                if text.contains("Completed") || text.contains("Cancelled") || text.contains("终止")
        )));
    }

    #[test]
    fn resume_legacy_completed_step_without_duration_still_has_terminal_notice() {
        let mut model = ConversationModel::default();

        ResumeConversation {
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "legacy-completed-run".into(),
                step_id: "legacy-completed-step".into(),
                messages: vec![TuiChatMessage::user_text("legacy completed question")],
                finalize_cause: Some(
                    crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::Completed,
                ),
                duration_ms: None,
            }],
        }
        .update(&mut model);

        assert_eq!(
            model
                .timeline
                .items()
                .iter()
                .filter(|item| matches!(
                    item,
                    OutputTimelineItem::System { text, .. }
                        if text.starts_with("✻ ")
                            && !text.contains(" for ")
                            && !text.contains("Completed")
                ))
                .count(),
            1
        );
    }

    #[test]
    fn resume_projects_cancelled_step_terminal_notice() {
        let mut model = ConversationModel::default();

        ResumeConversation {
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "cancelled-run".into(),
                step_id: "cancelled-step".into(),
                messages: vec![TuiChatMessage::user_text("cancelled question")],
                finalize_cause: Some(
                    crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep,
                ),
                duration_ms: Some(125_000),
            }],
        }
        .update(&mut model);

        assert_eq!(
            model
                .timeline
                .items()
                .iter()
                .filter(|item| matches!(
                    item,
                    OutputTimelineItem::System { text, .. }
                        if text == "✻ Cancelled, ran 2m 5s"
                ))
                .count(),
            1
        );
        assert!(!model.timeline.items().iter().any(|item| matches!(
            item,
            OutputTimelineItem::System { text, .. }
                if text.contains("Completed") || text.contains(" for ") || text.contains("终止")
        )));
    }

    #[test]
    fn resume_projects_reconstructed_unfinished_bash_as_error_and_terminated_notice() {
        let mut model = ConversationModel::default();
        let assistant = TuiChatMessage {
            role: "assistant".to_string(),
            content: vec![TuiContentBlock::ToolUse {
                id: "provider-call-1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "sleep 180"}),
            }],
            input_id: None,
            source: TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
        };
        let result = TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::ToolResult {
                tool_use_id: "provider-call-1".to_string(),
                content: serde_json::json!({"outcome": "CancellationUnconfirmed"}),
                is_error: true,
                text: Some("cleanup could not be confirmed".to_string()),
            }],
            input_id: None,
            source: TuiMessageSource::SystemGenerated,
            hook_notice: None,
            skill_request: None,
        };

        ResumeConversation {
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "terminated-run".into(),
                step_id: "running-tool-step".into(),
                messages: vec![assistant, result],
                finalize_cause: Some(
                    crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::RunTerminated,
                ),
                duration_ms: None,
            }],
        }
        .update(&mut model);

        let chat_id =
            crate::tui::model::conversation::ids::ChatId::from_legacy_or_new("terminated-run");
        let run_id = crate::tui::model::conversation::ids::ChatRunId::from_legacy_or_new(
            "running-tool-step",
        );
        let turn = model
            .chats
            .iter()
            .find(|chat| chat.id == chat_id)
            .and_then(|chat| chat.runs.iter().find(|turn| turn.id == run_id))
            .expect("恢复后应存在终止 Step");
        let call = turn.tool_calls.first().expect("恢复后应存在 Bash ToolCall");
        assert_eq!(call.name, "Bash");
        assert_eq!(call.status, ToolCallStatus::Error);
        assert!(model
            .timeline
            .items()
            .iter()
            .any(|item| matches!(item, OutputTimelineItem::ToolCall { .. })));
        assert!(model
            .timeline
            .items()
            .iter()
            .any(|item| matches!(item, OutputTimelineItem::ToolResult { .. })));
        assert_eq!(
            model
                .timeline
                .items()
                .iter()
                .filter(|item| matches!(
                    item,
                    OutputTimelineItem::System { text, .. } if text == "此 Run 已终止"
                ))
                .count(),
            1
        );
        assert!(!model.timeline.items().iter().any(|item| matches!(
            item,
            OutputTimelineItem::System { text, .. }
                if text.contains("Completed") || text.contains("Cancelled") || text.contains(" for ")
        )));
    }

    #[test]
    fn resume_conversation_equality_compares_step_identity_and_messages() {
        let first = ResumeConversation {
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "run-1".into(),
                step_id: "step-1".into(),
                messages: vec![TuiChatMessage::user_text("first")],
                finalize_cause: None,
                duration_ms: None,
            }],
        };
        let different = ResumeConversation {
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "run-2".into(),
                step_id: "step-1".into(),
                messages: vec![TuiChatMessage::user_text("second")],
                finalize_cause: None,
                duration_ms: None,
            }],
        };

        assert_ne!(first, different);
    }

    #[test]
    fn resume_restores_answered_ask_batches_in_assistant_message_order() {
        let assistant_one = TuiChatMessage {
            role: "assistant".to_string(),
            content: vec![ask_tool_use("ask-1", "第一问")],
            input_id: None,
            source: TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
        };
        let assistant_two = TuiChatMessage {
            role: "assistant".to_string(),
            content: vec![ask_tool_use("ask-2", "第二问")],
            input_id: None,
            source: TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
        };
        let mut model = ConversationModel::default();

        ResumeConversation {
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "history-run".into(),
                step_id: "history-step".into(),
                messages: vec![
                    assistant_one,
                    ask_result("ask-1", serde_json::json!({ "answer": "答案一" })),
                    assistant_two,
                    ask_result("ask-2", serde_json::json!("答案二")),
                ],
                finalize_cause: None,
                duration_ms: None,
            }],
        }
        .update(&mut model);

        let restored = model
            .timeline
            .items()
            .iter()
            .filter_map(|item| match item {
                OutputTimelineItem::AskUserBatch {
                    slots,
                    completion: crate::tui::model::conversation::block::AskUserCompletion::Answered,
                    ..
                } => Some((slots[0].question.as_str(), slots[0].answer.as_deref())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            restored,
            vec![("第一问", Some("答案一")), ("第二问", Some("答案二"))]
        );
    }
    #[test]
    fn resume_excludes_llm_only_messages_from_user_history() {
        let user = TuiChatMessage::user_text("user question");
        let hook_notice = TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text(
                "<system-reminder>blocked by hook</system-reminder>",
            )],
            input_id: None,
            source: TuiMessageSource::Hook,
            hook_notice: None,
            skill_request: None,
        };
        let system_generated = TuiChatMessage::system_generated_user_text(
            "<system-reminder>Skill loaded</system-reminder>",
        );
        let assistant = TuiChatMessage::assistant_text("assistant reply");
        let mut model = ConversationModel::default();

        ResumeConversation {
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "history-run".into(),
                step_id: "history-step".into(),
                messages: vec![user, hook_notice, system_generated, assistant],
                finalize_cause: None,
                duration_ms: None,
            }],
        }
        .update(&mut model);

        let user_messages = model
            .timeline
            .items()
            .iter()
            .filter_map(|item| match item {
                OutputTimelineItem::UserMessage { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(user_messages, ["user question"]);
        assert!(!model.timeline.items().iter().any(|item| match item {
            OutputTimelineItem::UserMessage { text, .. } => text.contains("system-reminder"),
            _ => false,
        }));
    }
}
