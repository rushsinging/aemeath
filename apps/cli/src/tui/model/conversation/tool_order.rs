// Timeline 工具顺序操作：push_tool_call_ref / push_tool_result_ref / move_tool_result_after_tool_call。

use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::model::ConversationModel;

impl ConversationModel {
    pub(super) fn insert_tool_call_block_before_active_text(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
    ) {
        // A4.3：存在性查询改读 timeline（原读 blocks）。
        if self
            .timeline
            .contains_tool_call(&chat_id, &turn_id, id.as_ref())
        {
            return;
        }
        self.timeline.push_tool_call_ref(chat_id, turn_id, id);
    }

    pub(super) fn insert_tool_result_after_tool_call(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
    ) {
        self.timeline.push_tool_result_ref(chat_id, turn_id, id);
    }

    pub(super) fn move_tool_results_after_tool_call(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        id: &str,
    ) {
        self.timeline.move_tool_result_after_tool_call(
            chat_id,
            turn_id,
            &ToolCallId::from_legacy_or_new(id),
        );
    }
}
