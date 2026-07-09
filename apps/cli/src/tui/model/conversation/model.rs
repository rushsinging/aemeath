use super::agent_progress::AgentProgressEntry;
use super::change::ConversationChange;
use super::chat::{Chat, ChatStatus};
use super::chat_turn::ChatTurn;
use super::ids::{ChatId, ChatTurnId};
use super::queued_submission::QueuedSubmission;
use super::runtime_state::RuntimeState;
use super::update::ConversationUpdate;
use crate::tui::app::event::ModelStreamWaitingView;
use crate::tui::model::output_timeline::{OutputTimelineItem, OutputTimelineModel};
use std::time::Instant;

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
    /// 供渲染层 memo `assemble_from_conversation`：revision 不变即可复用上次 view_model。
    revision: u64,
    pub(super) active_text_block_id: Option<String>,
    pub(super) active_text_context: Option<(ChatId, ChatTurnId)>,
    pub(super) active_thinking_block_id: Option<String>,
    pub(super) active_thinking_context: Option<(ChatId, ChatTurnId)>,
    pub model_stream_placeholder: Option<ModelStreamWaitingView>,

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
            active_text_block_id: None,
            active_text_context: None,
            active_thinking_block_id: None,
            active_thinking_context: None,
            model_stream_placeholder: None,
            runtime: RuntimeState::default(),
        }
    }
}

impl ConversationModel {
    /// 清空整段对话，回到初始空状态。用于 `/clear` 等需要重置单一真相源的场景。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn apply<U: ConversationUpdate>(&mut self, update: U) -> Vec<ConversationChange> {
        let changes = update.update(self);
        if !changes.is_empty() {
            self.revision = self.revision.wrapping_add(1);
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

    pub(super) fn clear_model_stream_placeholder(&mut self) -> Vec<ConversationChange> {
        if let Some(placeholder) = self.model_stream_placeholder.take() {
            crate::tui::log_debug!(
                "clear model_stream_placeholder chat_id={} turn_id={} elapsed_secs={} phase={}",
                placeholder.context.chat_id,
                placeholder.context.turn_id,
                placeholder.elapsed_secs,
                placeholder.phase,
            );
            vec![ConversationChange::OutputDirty]
        } else {
            Vec::new()
        }
    }

    pub(super) fn upsert_model_stream_placeholder(
        &mut self,
        placeholder: ModelStreamWaitingView,
    ) -> Vec<ConversationChange> {
        crate::tui::log_debug!(
            "upsert model_stream_placeholder chat_id={} turn_id={} elapsed_secs={} phase={}",
            placeholder.context.chat_id,
            placeholder.context.turn_id,
            placeholder.elapsed_secs,
            placeholder.phase,
        );
        self.model_stream_placeholder = Some(placeholder);
        vec![ConversationChange::OutputDirty]
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
            id: user_block_id,
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
        input_id: sdk::InputId,
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
        input_id: &sdk::InputId,
    ) -> Vec<ConversationChange> {
        let before = self.queued_submissions.len();
        self.queued_submissions.retain(|q| &q.input_id != input_id);
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
    pub fn sync_queued_from_runtime(&mut self, queued: &[sdk::ChatMessage]) {
        self.queued_submissions.clear();
        self.timeline
            .retain(|it| !matches!(it, OutputTimelineItem::QueuedUserMessage { .. }));
        for msg in queued {
            let id = self.next_block_id("queued");
            let input_id = msg
                .input_id
                .as_ref()
                .cloned()
                .unwrap_or_else(sdk::InputId::new_v7);
            let text = msg.text_content().to_string();
            self.queued_submissions.push(QueuedSubmission::new(
                id.clone(),
                input_id.clone(),
                text.clone(),
            ));
            self.timeline
                .push(OutputTimelineItem::QueuedUserMessage { id, input_id, text });
        }
        self.revision = self.revision.wrapping_add(1);
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
