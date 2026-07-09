use super::agent_progress::AgentProgressEntry;
use super::change::ConversationChange;
use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::model::ConversationModel;
use super::streaming_preview::{ToolStreamingPreviewBuffer, ToolStreamingPreviewPolicy};
use super::tool_call::{AgentMeta, ToolCall, ToolCallChange, ToolCallStatus};

const STREAM_CAP: usize = 4 * 1024;

fn push_streaming_preview_activity(call: &mut ToolCall, message: &str) {
    let policy = match call.name.as_str() {
        "Bash" => ToolStreamingPreviewPolicy::new(5, true, STREAM_CAP),
        "Agent" => ToolStreamingPreviewPolicy::new(5, true, STREAM_CAP),
        _ => return,
    };
    let buffer = call
        .streaming_preview
        .get_or_insert_with(|| ToolStreamingPreviewBuffer::new(policy));
    buffer.push_chunk(message);
    call.activities = buffer.display_lines();
}

pub(super) struct ToolCallUpdateObservation {
    pub(super) chat_id: ChatId,
    pub(super) turn_id: ChatTurnId,
    pub(super) id: ToolCallId,
    pub(super) provider_id: Option<String>,
    pub(super) name: String,
    pub(super) index: usize,
    pub(super) arguments: Option<String>,
    pub(super) status: ToolCallStatus,
}

impl ConversationModel {
    pub(super) fn start_tool_call(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
        _provider_id: Option<String>,
        name: String,
        index: usize,
    ) -> Vec<ConversationChange> {
        self.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        crate::tui::log_debug!(
            "model observe tool_call_start chat_id={} turn_id={} id={} name={} index={} timeline_items_before={}",
            chat_id,
            turn_id,
            id,
            name,
            index,
            self.timeline.items().len(),
        );
        let tool_call_id = id.clone();
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
            turn.observe_tool_start(tool_call_id.clone(), chat_id.clone(), name.clone(), index);
        }
        self.insert_tool_call_block_before_active_text(chat_id, turn_id, tool_call_id);
        vec![
            ConversationChange::ToolCallObserved { name, index },
            ConversationChange::OutputDirty,
        ]
    }
    pub(super) fn update_tool_call(
        &mut self,
        update: ToolCallUpdateObservation,
    ) -> Vec<ConversationChange> {
        let ToolCallUpdateObservation {
            chat_id,
            turn_id,
            id,
            provider_id,
            name,
            index,
            arguments,
            status,
        } = update;
        self.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        let mut candidate_ids = vec![Some(id.to_string())];
        if let Some(ref pid) = provider_id {
            let pid_as_uuid = ToolCallId::from_legacy_or_new(pid).to_string();
            if !candidate_ids.contains(&Some(pid_as_uuid.clone())) {
                candidate_ids.push(Some(pid_as_uuid));
            }
            candidate_ids.push(Some(pid.clone()));
        }
        let mut bound_id = id.clone();
        let mut args_preview = arguments.clone().unwrap_or_default();
        let mut bound = false;
        let mut running = false;
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
            for candidate_id in candidate_ids.into_iter().flatten() {
                if let Some((preview, changes)) =
                    turn.update_tool(&candidate_id, arguments.clone(), status)
                {
                    args_preview = preview;
                    bound_id = ToolCallId::from_legacy_or_new(&candidate_id);
                    running = changes.contains(&ToolCallChange::Running);

                    bound = true;
                    break;
                }
            }
        }
        if !bound {
            if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
                turn.observe_tool_start(id.clone(), chat_id.clone(), name.clone(), index);
                running = turn
                    .update_tool(id.as_ref(), arguments.clone(), status)
                    .is_some_and(|(_, changes)| changes.contains(&ToolCallChange::Running));
                bound_id = id.clone();
            }
        }
        self.promote_orphan_tool_result(&chat_id, &turn_id, bound_id.as_ref());
        // A4.3：存在性查询改读 timeline（原读 blocks.iter().position）。
        let tool_already_in_timeline =
            self.timeline
                .contains_tool_call(&chat_id, &turn_id, bound_id.as_ref());
        if !tool_already_in_timeline {
            self.insert_tool_call_block_before_active_text(
                chat_id.clone(),
                turn_id.clone(),
                bound_id.clone(),
            );
        }
        self.move_tool_results_after_tool_call(&chat_id, &turn_id, bound_id.as_ref());
        crate::tui::log_trace!(
            "model bound tool_call_update chat_id={} turn_id={} id={} provider_id={:?} bound_id={} name={} index={} status={:?} bound={} args_len={} has_block={} timeline_items_after={}",
            chat_id,
            turn_id,
            id,
            provider_id,
            bound_id,
            name,
            index,
            status,
            bound,
            args_preview.len(),
            tool_already_in_timeline,
            self.timeline.items().len(),
        );
        vec![
            ConversationChange::ToolCallBound {
                id: bound_id.to_string(),
                name,
                running,
            },
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn record_agent_progress(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        tool_id: ToolCallId,
        message: String,
    ) -> Vec<ConversationChange> {
        // Maximum bytes of accumulated stdout to retain for live display.
        // Older content is trimmed to keep memory bounded for high-volume output.
        const STREAM_CAP: usize = 4 * 1024;

        // 查找匹配的 ToolCall，将进度信息写入其 activities（供 ToolCallBlock 渲染
        // activity_lines），而不是作为独立根级 AgentProgress block 泄露到对话流中。
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
            if let Some(call) = turn.tool_calls.iter_mut().find(|c| {
                c.id.as_ref()
                    .is_some_and(|id| id.as_ref() == tool_id.to_string())
            }) {
                if call.name == "Bash" || call.name == "Agent" {
                    push_streaming_preview_activity(call, &message);
                } else {
                    call.activities.push(message.clone());
                }
            }
        }
        self.agent_progress.push(AgentProgressEntry::new(
            tool_id.to_string(),
            message.clone(),
        ));
        vec![ConversationChange::OutputDirty]
    }

    /// 写入 Agent 工具的 role/model 元数据（issue #499）。
    ///
    /// 由 `AgentProgressKind::Started` 事件触发。仅当 ToolCall 存在且
    /// `agent_meta` 尚未设置时才写入，避免重复覆盖。
    pub(super) fn update_agent_meta(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        tool_id: ToolCallId,
        role: Option<String>,
        model: String,
    ) -> Vec<ConversationChange> {
        let mut changes = Vec::new();
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
            if let Some(call) = turn.tool_calls.iter_mut().find(|c| {
                c.id.as_ref()
                    .is_some_and(|id| id.as_ref() == tool_id.to_string())
            }) {
                if call.agent_meta.is_none() {
                    call.agent_meta = Some(AgentMeta { role, model });
                    changes.push(ConversationChange::AgentMetaUpdated {
                        tool_id: tool_id.to_string(),
                    });
                    changes.push(ConversationChange::OutputDirty);
                }
            }
        }
        changes
    }
}
