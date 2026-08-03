use super::activity_observation::{ActivityIncrementOutcome, ActivityObservationModel};
use super::agent_progress::AgentProgressEntry;
use super::change::ConversationChange;
use super::chat::{Chat, ChatStatus};
use super::chat_turn::ChatTurn;
use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::interaction::InteractionState;
use super::output_view_change::{
    OutputViewChange, OutputViewChanges, OutputViewCursor, OutputViewJournal,
};
use super::queued_submission::QueuedSubmission;
use super::runtime_state::RuntimeState;
use super::update::ConversationUpdate;
use crate::tui::model::output_timeline::{OutputTimelineItem, OutputTimelineModel};
use std::time::Instant;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ConversationRetainedStateSnapshot {
    pub chats: usize,
    pub turns: usize,
    pub tool_calls: usize,
    pub timeline_items: usize,
    pub agent_progress_entries: usize,
    pub agent_progress_bytes: usize,
    pub output_view_journal_entries: usize,
    pub output_view_journal_item_id_bytes: usize,
    pub has_active_interaction: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConversationModel {
    // ── 对话内容 ──
    pub chats: Vec<Chat>,
    pub active_chat_id: Option<ChatId>,
    pub timeline: OutputTimelineModel,
    pub queued_submissions: Vec<QueuedSubmission>,
    pub agent_progress: Vec<AgentProgressEntry>,
    next_chat_sequence: usize,
    next_block_sequence: usize,
    /// 单调递增的内容版本号；每次产生 change 的 apply +1。
    /// 供输出窗口同步判断当前物化结果是否仍可复用。
    revision: u64,
    output_view_journal: OutputViewJournal,
    pub(super) active_text_block_id: Option<String>,
    pub(super) active_text_context: Option<(ChatId, ChatTurnId)>,
    pub(super) active_thinking_block_id: Option<String>,
    pub(super) active_thinking_context: Option<(ChatId, ChatTurnId)>,
    pub(super) active_interaction: Option<InteractionState>,
    activity_observations: ActivityObservationModel,

    // ── 运行态 ──
    pub runtime: RuntimeState,
}
#[allow(clippy::derivable_impls)]
impl Default for ConversationModel {
    fn default() -> Self {
        Self {
            chats: Vec::new(),
            active_chat_id: None,
            timeline: OutputTimelineModel::default(),
            queued_submissions: Vec::new(),
            agent_progress: Vec::new(),
            next_chat_sequence: 0,
            next_block_sequence: 0,
            revision: 0,
            output_view_journal: OutputViewJournal::default(),
            active_text_block_id: None,
            active_text_context: None,
            active_thinking_block_id: None,
            active_thinking_context: None,
            active_interaction: None,
            activity_observations: ActivityObservationModel::default(),
            runtime: RuntimeState::default(),
        }
    }
}

impl ConversationModel {
    /// 清空整段对话，回到初始空状态。用于 `/clear` 等需要重置单一真相源的场景。
    pub fn reset(&mut self) {
        let mut journal = std::mem::take(&mut self.output_view_journal);
        *self = Self::default();
        journal.publish(OutputViewChange::Reset);
        self.output_view_journal = journal;
    }

    pub fn apply<U: ConversationUpdate>(&mut self, update: U) -> Vec<ConversationChange> {
        let changes = update.update(self);
        if !changes.is_empty() {
            self.revision = self.revision.wrapping_add(1);
            self.publish_output_view_changes(&changes);
        } else {
            let _ = self.timeline.take_pending_view_changes();
        }
        changes
    }

    /// 检查临时 notice 是否过期；过期则回退到 graph_phase 派生的持久态。
    /// 返回 `true` 表示发生了回退（调用方可据此标脏）。
    pub fn expire_transient_notice(&mut self, now: Instant) -> bool {
        self.runtime.expire_transient_notice(now)
    }

    /// 当前内容版本号，供渲染层 memo。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn output_view_cursor(&self) -> OutputViewCursor {
        self.output_view_journal.cursor()
    }

    pub(crate) fn output_view_changes_since(&self, cursor: OutputViewCursor) -> OutputViewChanges {
        self.output_view_journal.changes_since(cursor)
    }

    pub(crate) fn tool_call(
        &self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        tool_call_id: &ToolCallId,
    ) -> Option<&super::tool_call::ToolCall> {
        self.chats
            .iter()
            .find(|chat| &chat.id == chat_id)?
            .turns
            .iter()
            .find(|turn| &turn.id == turn_id)?
            .tool_calls
            .iter()
            .find(|call| call.id.as_ref() == Some(tool_call_id))
    }

    fn publish_output_view_changes(&mut self, changes: &[ConversationChange]) {
        let timeline_changes = self.timeline.take_pending_view_changes();
        let mut changed_item_ids = std::collections::HashSet::new();
        for change in &timeline_changes {
            match change {
                OutputViewChange::Append { item_id }
                | OutputViewChange::Update { item_id }
                | OutputViewChange::Remove { item_id } => {
                    changed_item_ids.insert(item_id.clone());
                }
                OutputViewChange::Reset => {}
            }
            self.output_view_journal.publish(change.clone());
        }

        for item_id in changes.iter().filter_map(output_view_item_id_for_change) {
            if !changed_item_ids.contains(&item_id) {
                self.output_view_journal
                    .publish(OutputViewChange::Update { item_id });
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn retained_state_snapshot(&self) -> ConversationRetainedStateSnapshot {
        let turns = self.chats.iter().map(|chat| chat.turns.len()).sum();
        let tool_calls = self
            .chats
            .iter()
            .flat_map(|chat| &chat.turns)
            .map(|turn| turn.tool_calls.len())
            .sum();
        let agent_progress_bytes = self
            .agent_progress
            .iter()
            .map(|entry| entry.tool_id.len().saturating_add(entry.message.len()))
            .sum();
        let (output_view_journal_entries, output_view_journal_item_id_bytes) =
            self.output_view_journal.retained_metrics();

        ConversationRetainedStateSnapshot {
            chats: self.chats.len(),
            turns,
            tool_calls,
            timeline_items: self.timeline.items().len(),
            agent_progress_entries: self.agent_progress.len(),
            agent_progress_bytes,
            output_view_journal_entries,
            output_view_journal_item_id_bytes,
            has_active_interaction: self.active_interaction.is_some(),
        }
    }

    pub(crate) fn activity_observations(&self) -> &ActivityObservationModel {
        &self.activity_observations
    }

    #[cfg(test)]
    pub(crate) fn activity_observations_mut(&mut self) -> &mut ActivityObservationModel {
        &mut self.activity_observations
    }

    pub(super) fn observe_activity_change(
        &mut self,
        activity: crate::tui::adapter::tui_runtime_event::TuiActivityObservation,
    ) -> Vec<ConversationChange> {
        let run_id = activity.run_id.clone();
        let activity_id = activity.id.clone();
        match self.activity_observations.observe_increment(activity) {
            ActivityIncrementOutcome::Applied => {
                vec![ConversationChange::ActivityObservationChanged {
                    run_id,
                    activity_id,
                }]
            }
            ActivityIncrementOutcome::GapDetected => {
                vec![ConversationChange::ActivityObservationStale { run_id }]
            }
            ActivityIncrementOutcome::Ignored => Vec::new(),
        }
    }

    pub(super) fn replace_activity_snapshot(
        &mut self,
        snapshot: crate::tui::adapter::tui_runtime_event::TuiActivitySnapshot,
    ) -> Vec<ConversationChange> {
        let run_id = snapshot.run_id.clone();
        if self.activity_observations.replace_snapshot(snapshot) {
            vec![ConversationChange::ActivitySnapshotReplaced { run_id }]
        } else {
            Vec::new()
        }
    }

    pub(super) fn start_chat(&mut self, submission: String) -> Vec<ConversationChange> {
        self.next_chat_sequence += 1;
        let chat_id = ChatId::new_v7();
        let chat = Chat::new(chat_id.clone(), submission.clone());
        self.active_chat_id = Some(chat_id.clone());
        self.chats.push(chat);
        let user_block_id = self.next_block_id("user");
        let turn_id = ChatTurnId::new_v7();
        self.timeline.push(OutputTimelineItem::UserMessage {
            id: user_block_id.clone(),
            text: submission,
        });
        vec![
            ConversationChange::ChatStarted {
                chat_id: chat_id.to_string(),
            },
            ConversationChange::ChatTurnStarted {
                chat_id: chat_id.to_string(),
                turn_id: turn_id.to_string(),
            },
            ConversationChange::UserMessageAppended {
                block_id: user_block_id,
            },
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn append_user_message(&mut self, text: String) -> Vec<ConversationChange> {
        let block_id = self.next_block_id("user");
        self.timeline.push(OutputTimelineItem::UserMessage {
            id: block_id.clone(),
            text,
        });
        vec![
            ConversationChange::UserMessageAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    pub(crate) fn ensure_runtime_turn(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
    ) -> (ChatId, ChatTurnId) {
        if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
            chat.status = ChatStatus::Running;
            if !chat.turns.iter().any(|turn| turn.id == turn_id) {
                let sequence = chat.turns.len();
                chat.turns.push(ChatTurn::new(turn_id.clone(), sequence));
            }
            return (chat_id, turn_id);
        }
        let mut chat = Chat::new(chat_id.clone(), String::new());
        chat.turns.clear();
        chat.turns.push(ChatTurn::new(turn_id.clone(), 0));
        self.chats.push(chat);
        (chat_id, turn_id)
    }

    pub(super) fn runtime_turn_mut(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
    ) -> Option<&mut ChatTurn> {
        self.chats
            .iter_mut()
            .find(|chat| &chat.id == chat_id)
            .and_then(|chat| chat.turns.iter_mut().find(|turn| &turn.id == turn_id))
    }

    pub(super) fn complete_chat(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
    ) -> Vec<ConversationChange> {
        self.active_text_block_id = None;
        self.active_text_context = None;
        self.active_thinking_block_id = None;
        self.active_thinking_context = None;
        let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) else {
            return Vec::new();
        };
        if !chat.turns.iter().any(|turn| turn.id == turn_id) {
            return Vec::new();
        }
        chat.status = ChatStatus::Completing;
        let chat_id = chat.id.as_ref().to_string();
        vec![ConversationChange::ChatCompleting { chat_id }]
    }

    pub(super) fn queue_submission(
        &mut self,
        input_id: String,
        text: String,
    ) -> Vec<ConversationChange> {
        let id = self.next_block_id("queued");
        self.queued_submissions.push(QueuedSubmission::new(
            id.clone(),
            input_id.clone(),
            text.clone(),
        ));
        self.timeline.push(OutputTimelineItem::QueuedUserMessage {
            id: id.clone(),
            input_id,
            text,
        });
        vec![
            ConversationChange::QueuedSubmissionAdded { id },
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn clear_queued_submission_by_id(
        &mut self,
        input_id: &str,
    ) -> Vec<ConversationChange> {
        let before = self.queued_submissions.len();
        self.queued_submissions.retain(|q| q.input_id != input_id);
        self.timeline.retain(|it| {
            !matches!(it,
                OutputTimelineItem::QueuedUserMessage { input_id: tid, .. } if tid == input_id)
        });
        let removed = before - self.queued_submissions.len();
        vec![
            ConversationChange::QueuedSubmissionsCleared { count: removed },
            ConversationChange::OutputDirty,
        ]
    }

    /// 批量清空所有排队中的提交占位（#391 S3）。
    pub(super) fn clear_all_queued_submissions(&mut self) -> Vec<ConversationChange> {
        let removed = self.queued_submissions.len();
        self.queued_submissions.clear();
        self.timeline
            .retain(|it| !matches!(it, OutputTimelineItem::QueuedUserMessage { .. }));
        vec![
            ConversationChange::QueuedSubmissionsCleared { count: removed },
            ConversationChange::OutputDirty,
        ]
    }

    /// 以 runtime 返回的全量 queued 快照为准重渲染 queue 区域。
    pub(super) fn sync_queued_submissions(
        &mut self,
        queued: Vec<crate::tui::adapter::runtime_view::TuiChatMessage>,
    ) -> Vec<ConversationChange> {
        self.queued_submissions.clear();
        self.timeline
            .retain(|it| !matches!(it, OutputTimelineItem::QueuedUserMessage { .. }));
        for msg in queued {
            let id = self.next_block_id("queued");
            let input_id = match msg.input_id.as_ref() {
                Some(id) => id.clone(),
                None => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[tui] sync_queued_from_runtime: ChatMessage missing input_id, not projecting to queue"
                    );
                    continue;
                }
            };
            let text = msg.text_content().to_string();
            self.queued_submissions.push(QueuedSubmission::new(
                id.clone(),
                input_id.clone(),
                text.clone(),
            ));
            self.timeline
                .push(OutputTimelineItem::QueuedUserMessage { id, input_id, text });
        }
        vec![ConversationChange::QueuedSubmissionsSynced {
            count: self.queued_submissions.len(),
        }]
    }

    pub(super) fn clear_compact_runtime(&mut self) -> Vec<ConversationChange> {
        self.runtime.clear_compact_runtime();
        vec![ConversationChange::CompactRuntimeCleared]
    }

    pub(super) fn next_block_id(&mut self, prefix: &str) -> String {
        self.next_block_sequence += 1;
        format!("{prefix}-{}", self.next_block_sequence)
    }

    pub(super) fn active_chat_mut(&mut self) -> Option<&mut Chat> {
        let active = self.active_chat_id.clone()?;
        self.chats.iter_mut().find(|chat| chat.id == active)
    }
}

fn output_view_item_id_for_change(change: &ConversationChange) -> Option<String> {
    match change {
        ConversationChange::UserMessageAppended { block_id }
        | ConversationChange::AssistantTextAppended { block_id }
        | ConversationChange::ThinkingTextAppended { block_id }
        | ConversationChange::SystemMessageAppended { block_id }
        | ConversationChange::ErrorAppended { block_id, .. }
        | ConversationChange::QueuedSubmissionAdded { id: block_id }
        | ConversationChange::BlockCompleted {
            block_id: Some(block_id),
        }
        | ConversationChange::AskUserShown { id: block_id }
        | ConversationChange::AskUserUpdated { id: block_id }
        | ConversationChange::OrphanToolResultObserved { id: block_id }
        | ConversationChange::AgentProgressRecorded {
            block_id,
            tool_id: _,
        } => Some(block_id.clone()),
        ConversationChange::AskUserDismissed { .. } => None,
        ConversationChange::ToolCallObserved { .. } => None,
        ConversationChange::OutputDirty => None,
        ConversationChange::ToolCallBound {
            chat_id,
            turn_id,
            id,
            ..
        }
        | ConversationChange::ToolCallCompleted {
            chat_id,
            turn_id,
            id,
            ..
        }
        | ConversationChange::AgentMetaUpdated {
            chat_id,
            turn_id,
            tool_id: id,
        } => Some(format!("tool-call-{chat_id}/{turn_id}/{id}")),
        _ => None,
    }
}

#[cfg(test)]
use super::output_view_change::OUTPUT_VIEW_JOURNAL_CAPACITY;

#[cfg(test)]
#[path = "output_view_change_tests.rs"]
mod output_view_change_tests;

#[cfg(test)]
mod tests {
    use super::super::intent::*;
    use super::*;

    #[test]
    fn test_revision_starts_at_zero() {
        let model = ConversationModel::default();
        assert_eq!(model.revision(), 0, "新建 conversation revision 应为 0");
    }

    #[test]
    fn test_revision_bumps_on_mutating_apply() {
        let mut model = ConversationModel::default();
        let before = model.revision();
        let changes = model.apply(AppendUserMessage {
            text: "你好".to_string(),
        });
        assert!(!changes.is_empty(), "AppendUserMessage 应产生 change");
        assert_eq!(
            model.revision(),
            before + 1,
            "产生 change 的 apply 应使 revision +1"
        );
    }

    #[test]
    fn test_revision_unchanged_on_noop_apply() {
        let mut model = ConversationModel::default();
        let before = model.revision();
        // 空文本的 AssistantText 返回空 change（no-op）。
        let changes = model.apply(AssistantText {
            chat_id: ChatId::new("c1"),
            turn_id: ChatTurnId::new("t1"),
            text: String::new(),
        });
        assert!(changes.is_empty(), "空文本 AssistantText 应为 no-op");
        assert_eq!(model.revision(), before, "no-op apply 不应改 revision");
    }
}
