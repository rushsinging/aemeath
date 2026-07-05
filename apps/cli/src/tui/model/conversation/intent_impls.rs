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
                                    model_id: None,
                                    role: None,
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
            self.model_id,
            self.role,
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

impl ConversationUpdate for CompleteChat {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.complete_chat(self.chat_id, self.turn_id)
    }
}

// ════════════════════════════════════════════════════════════════════
//  Runtime intent impls（逻辑从 RuntimeModel::apply 搬入，
//  操作 ConversationModel 字段，返回 ConversationChange）
// ════════════════════════════════════════════════════════════════════

impl ConversationUpdate for SetProviderModel {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.provider = self.provider.clone();
        model.runtime.model_id = self.model_id.clone();
        vec![ConversationChange::ProviderModelChanged {
            provider: self.provider,
            model_id: self.model_id,
        }]
    }
}

impl ConversationUpdate for UpdateWorkspace {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.workspace.cwd = Some(self.cwd.clone());
        model.runtime.workspace.worktree = self.worktree.clone();
        vec![ConversationChange::WorkspaceChanged {
            cwd: self.cwd,
            worktree: self.worktree,
        }]
    }
}

impl ConversationUpdate for WorkspaceSnapshotReceived {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.workspace.path_base = self.path_base.clone();
        model.runtime.workspace.workspace_root = self.workspace_root.clone();
        model.runtime.workspace.branch = self.branch.clone();
        model.runtime.workspace.kind = self.kind;
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

impl ConversationUpdate for SetContextSize {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.usage.context_size = self.0;
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

impl ConversationUpdate for SetThinking {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange> {
        model.runtime.thinking = self.0;
        vec![ConversationChange::ThinkingChanged]
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
