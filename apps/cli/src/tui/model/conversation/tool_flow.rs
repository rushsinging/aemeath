use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::ids::ToolCallId;
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;

impl ConversationModel {
    pub(super) fn promote_orphan_tool_result(&mut self, id: &str) {
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
            output,
            is_error,
        } = self.blocks.remove(position)
        else {
            return;
        };
        if self
            .complete_active_tool(id, output.clone(), is_error)
            .is_some()
        {
            self.insert_tool_result_after_tool_call(
                ToolCallId::new(id.to_string()),
                output,
                is_error,
                0,
            );
        }
    }

    pub(super) fn observe_tool_result(
        &mut self,
        id: String,
        _tool_name: String,
        output: String,
        is_error: bool,
        image_count: usize,
    ) -> Vec<ConversationChange> {
        if let Some(status) = self.complete_active_tool(&id, output.clone(), is_error) {
            self.insert_tool_result_after_tool_call(
                ToolCallId::new(id.clone()),
                output,
                is_error,
                image_count,
            );
            return vec![
                ConversationChange::ToolCallCompleted { id, status },
                ConversationChange::StyleBoundaryResetRequired,
                ConversationChange::OutputDirty,
            ];
        }
        self.blocks.push(ConversationBlock::OrphanToolResult {
            id: id.clone(),
            output,
            is_error,
        });
        vec![
            ConversationChange::OrphanToolResultObserved { id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn complete_active_tool(
        &mut self,
        id: &str,
        output: String,
        is_error: bool,
    ) -> Option<ToolCallStatus> {
        let chat = self.active_chat_mut()?;
        let turn = chat.active_turn_mut()?;
        turn.complete_tool(id, output, is_error)
    }
}
