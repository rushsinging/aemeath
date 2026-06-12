use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;

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
            .complete_tool_in_context(chat_id, turn_id, id, output.clone(), is_error)
            .is_some()
        {
            self.insert_tool_result_after_tool_call(
                chat_id.clone(),
                turn_id.clone(),
                ToolCallId::new(id.to_string()),
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
        id: String,
        _provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    ) -> Vec<ConversationChange> {
        self.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        if let Some(status) =
            self.complete_tool_in_context(&chat_id, &turn_id, &id, output.clone(), is_error)
        {
            self.insert_tool_result_after_tool_call(
                chat_id.clone(),
                turn_id.clone(),
                ToolCallId::new(id.clone()),
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
                ConversationChange::ToolCallCompleted { id, status },
                ConversationChange::StyleBoundaryResetRequired,
                ConversationChange::OutputDirty,
            ];
        }
        self.blocks.push(ConversationBlock::OrphanToolResult {
            id: id.clone(),
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
            ConversationChange::OrphanToolResultObserved { id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }
    pub(super) fn complete_tool_in_context(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        id: &str,
        output: String,
        is_error: bool,
    ) -> Option<ToolCallStatus> {
        self.chats
            .iter_mut()
            .find(|chat| &chat.id == chat_id)
            .and_then(|chat| chat.turns.iter_mut().find(|turn| &turn.id == turn_id))
            .and_then(|turn| turn.complete_tool(id, output, is_error))
    }
}
