// Tool call 在 blocks 中的位置管理（插入、排序）。

use super::block::ConversationBlock;
use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::model::ConversationModel;

impl ConversationModel {
    pub(super) fn insert_tool_call_block_before_active_text(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
        name: String,
        summary: String,
        args_preview: String,
    ) {
        if self.blocks.iter().any(|block| {
            matches!(
                block,
                ConversationBlock::ToolCall { id: existing, chat_id: block_chat_id, turn_id: block_turn_id, .. }
                    if block_chat_id == &chat_id && block_turn_id == &turn_id && existing == &id
            )
        }) {
            return;
        }
        let block = ConversationBlock::ToolCall {
            id,
            chat_id,
            turn_id,
            name,
            summary,
            args_preview,
        };
        self.blocks.push(block);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn insert_tool_result_after_tool_call(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    ) {
        self.blocks.retain(|existing| {
            !matches!(
                existing,
                ConversationBlock::ToolResult { id: result_id, chat_id: result_chat_id, turn_id: result_turn_id, .. }
                    if result_chat_id == &chat_id && result_turn_id == &turn_id && result_id == &id
            )
        });
        let block = ConversationBlock::ToolResult {
            id: id.clone(),
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            output,
            content,
            is_error,
            image_count,
        };
        let Some(position) = self.blocks.iter().position(|existing| {
            matches!(
                existing,
                ConversationBlock::ToolCall { id: tool_id, chat_id: tool_chat_id, turn_id: tool_turn_id, .. }
                    if tool_chat_id == &chat_id && tool_turn_id == &turn_id && tool_id == &id
            )
        }) else {
            self.blocks.push(block);
            return;
        };
        self.blocks.insert(position + 1, block);
    }

    pub(super) fn move_tool_results_after_tool_call(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        id: &str,
    ) {
        let mut results = Vec::new();
        let mut index = 0;
        while index < self.blocks.len() {
            let should_move = matches!(
                &self.blocks[index],
                ConversationBlock::ToolResult { id: result_id, chat_id: result_chat_id, turn_id: result_turn_id, .. }
                    if result_chat_id == chat_id && result_turn_id == turn_id && result_id.as_ref() == id
            );
            if should_move {
                results.push(self.blocks.remove(index));
            } else {
                index += 1;
            }
        }
        for result in results.into_iter().rev() {
            let Some(position) = self.blocks.iter().position(|existing| {
                matches!(
                    existing,
                    ConversationBlock::ToolCall { id: tool_id, chat_id: tool_chat_id, turn_id: tool_turn_id, .. }
                        if tool_chat_id == chat_id && tool_turn_id == turn_id && tool_id.as_ref() == id
                )
            }) else {
                self.blocks.push(result);
                continue;
            };
            self.blocks.insert(position + 1, result);
        }
    }
}
