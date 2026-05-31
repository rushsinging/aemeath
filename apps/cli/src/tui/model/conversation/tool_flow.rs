use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::ids::ToolCallId;
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;
use crate::tui::render::display::safe_text::safe_str_slice_by_char;

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
        log::warn!("[orphan-diag] promote_orphan_tool_result FOUND id={id} -> promoting");
        let ConversationBlock::OrphanToolResult {
            id: _,
            tool_name: _,
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
        } else {
            log::warn!("[orphan-diag] promote_orphan_tool_result FAIL: complete_active_tool returned None for id={id}");
        }
    }

    pub(super) fn observe_tool_result(
        &mut self,
        id: String,
        tool_name: String,
        output: String,
        is_error: bool,
        image_count: usize,
    ) -> Vec<ConversationChange> {
        log::warn!(
            "[orphan-diag] observe_tool_result ENTRY id={} tool_name={} output_len={} is_error={} chats={}",
            id,
            tool_name,
            output.len(),
            is_error,
            self.chats.len(),
        );
        if let Some(status) = self.complete_active_tool(&id, output.clone(), is_error) {
            log::warn!(
                "[orphan-diag] observe_tool_result EMBEDDED id={} status={status:?}",
                id
            );
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
        log::warn!(
            "[orphan-diag] observe_tool_result ORPHAN id={} tool_name={} output_preview={}",
            id,
            tool_name,
            safe_str_slice_by_char(&output, 0, 200),
        );
        self.blocks.push(ConversationBlock::OrphanToolResult {
            id: id.clone(),
            tool_name,
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
        let Some(chat) = self.active_chat_mut() else {
            log::warn!(
                "[orphan-diag] complete_active_tool FAIL: no active_chat id={}",
                id
            );
            return None;
        };
        let Some(turn) = chat.active_turn_mut() else {
            log::warn!(
                "[orphan-diag] complete_active_tool FAIL: no active_turn id={}",
                id
            );
            return None;
        };
        log::warn!(
            "[orphan-diag] complete_active_tool searching id={} tool_calls_in_turn={} bound_ids={:?}",
            id,
            turn.tool_calls.len(),
            turn.tool_calls.iter().filter_map(|c| c.id.as_ref().map(|i| i.as_ref())).collect::<Vec<_>>(),
        );
        let result = turn.complete_tool(id, output, is_error);
        if result.is_none() {
            log::warn!(
                "[orphan-diag] complete_active_tool NOT_FOUND: id={} not in tool_calls",
                id
            );
        }
        result
    }
}
