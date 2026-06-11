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
            tool_name: _,
            output,
            content,
            is_error,
        } = self.blocks.remove(position)
        else {
            return;
        };
        if self
            .complete_tool_in_context(id, output.clone(), is_error)
            .is_some()
        {
            self.insert_tool_result_after_tool_call(
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
        id: String,
        _provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    ) -> Vec<ConversationChange> {
        if let Some(status) = self.complete_tool_in_context(&id, output.clone(), is_error) {
            self.insert_tool_result_after_tool_call(
                ToolCallId::new(id.clone()),
                output,
                content,
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
            tool_name,
            output,
            content,
            is_error,
        });
        vec![
            ConversationChange::OrphanToolResultObserved { id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }
    pub(super) fn complete_tool_in_context(
        &mut self,
        id: &str,
        output: String,
        is_error: bool,
    ) -> Option<ToolCallStatus> {
        for chat in &mut self.chats {
            if let Some(turn) = chat.turns.iter_mut().find(|turn| {
                turn.tool_calls.iter().any(|call| {
                    call.id
                        .as_ref()
                        .is_some_and(|call_id| call_id.as_ref() == id)
                })
            }) {
                return turn.complete_tool(id, output, is_error);
            }
        }
        None
    }
}
