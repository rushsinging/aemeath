use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;
use super::tool_result_payload::ToolResultPayload;
use crate::tui::model::output_timeline::OutputTimelineItem;

impl ConversationModel {
    pub(super) fn promote_orphan_tool_result(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        id: &str,
    ) {
        let Some(position) = self.blocks.iter().position(|block| {
            matches!(
                block,
                ConversationBlock::OrphanToolResult { id: orphan_id, .. } if orphan_id == id
            )
        }) else {
            return;
        };
        let ConversationBlock::OrphanToolResult {
            id: _,
            tool_name: _,
            output,
            content,
            is_error,
        } = self.blocks.remove(position)
        else {
            return;
        };
        if self
            .complete_tool_in_context(
                chat_id,
                turn_id,
                id,
                ToolResultPayload::new(output.clone(), content.clone(), is_error, 0),
            )
            .is_some()
        {
            self.timeline.retain(|item| {
                !matches!(item, OutputTimelineItem::OrphanToolResult { id: orphan_id, .. } if orphan_id == id)
            });
            self.insert_tool_result_after_tool_call(
                chat_id.clone(),
                turn_id.clone(),
                ToolCallId::from_legacy_or_new(id),
                output,
                content,
                is_error,
                0,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn observe_tool_result(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
        _provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    ) -> Vec<ConversationChange> {
        self.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        if let Some(status) = self.complete_tool_in_context(
            &chat_id,
            &turn_id,
            id.as_ref(),
            ToolResultPayload::new(output.clone(), content.clone(), is_error, image_count),
        ) {
            self.insert_tool_result_after_tool_call(
                chat_id.clone(),
                turn_id.clone(),
                id.clone(),
                output,
                content,
                is_error,
                image_count,
            );
            log::debug!(
                target: "cli::tui::tool_flow",
                "model observe tool_result embedded id={} tool_name={} status={:?} is_error={} image_count={} blocks_after={}",
                id,
                tool_name,
                status,
                is_error,
                image_count,
                self.blocks.len(),
            );
            return vec![
                ConversationChange::ToolCallCompleted {
                    id: id.to_string(),
                    status,
                },
                ConversationChange::StyleBoundaryResetRequired,
                ConversationChange::OutputDirty,
            ];
        }
        self.timeline.push(OutputTimelineItem::OrphanToolResult {
            id: id.to_string(),
            tool_name: tool_name.clone(),
            output: output.clone(),
            content: content.clone(),
            is_error,
        });
        self.blocks.push(ConversationBlock::OrphanToolResult {
            id: id.to_string(),
            tool_name,
            output,
            content,
            is_error,
        });
        log::debug!(
            target: "cli::tui::tool_flow",
            "model observe tool_result orphan id={} is_error={} image_count={} blocks_after={}",
            id,
            is_error,
            image_count,
            self.blocks.len(),
        );
        vec![
            ConversationChange::OrphanToolResultObserved { id: id.to_string() },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }
    pub(super) fn complete_tool_in_context(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        id: &str,
        result: ToolResultPayload,
    ) -> Option<ToolCallStatus> {
        self.chats
            .iter_mut()
            .find(|chat| &chat.id == chat_id)
            .and_then(|chat| chat.turns.iter_mut().find(|turn| &turn.id == turn_id))
            .and_then(|turn| turn.complete_tool(id, result))
    }
}
