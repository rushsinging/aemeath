// Tool call 在 blocks 中的位置管理（插入、排序）。

use super::block::ConversationBlock;
use super::ids::ToolCallId;
use super::model::ConversationModel;

impl ConversationModel {
    pub(super) fn insert_tool_call_block_before_active_text(
        &mut self,
        id: ToolCallId,
        name: String,
        summary: String,
        args_preview: String,
    ) {
        let block = ConversationBlock::ToolCall {
            id,
            name,
            summary,
            args_preview,
        };
        let Some(position) = self
            .active_text_block_id()
            .and_then(|text_id| self.blocks.iter().position(|b| b.id() == text_id))
        else {
            self.blocks.push(block);
            return;
        };
        self.blocks.insert(position, block);
    }

    pub(super) fn insert_tool_result_after_tool_call(
        &mut self,
        id: ToolCallId,
        output: String,
        is_error: bool,
        image_count: usize,
    ) {
        let block = ConversationBlock::ToolResult {
            id: id.clone(),
            output,
            is_error,
            image_count,
        };
        let Some(position) = self.blocks.iter().position(|existing| {
            matches!(
                existing,
                ConversationBlock::ToolCall { id: tool_id, .. } if tool_id == &id
            )
        }) else {
            self.blocks.push(block);
            return;
        };
        self.blocks.insert(position + 1, block);
    }

    pub(super) fn move_tool_results_after_tool_call(&mut self, id: &str) {
        let mut results = Vec::new();
        let mut index = 0;
        while index < self.blocks.len() {
            let should_move = matches!(
                &self.blocks[index],
                ConversationBlock::ToolResult { id: result_id, .. } if result_id.as_ref() == id
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
                    ConversationBlock::ToolCall { id: tool_id, .. } if tool_id.as_ref() == id
                )
            }) else {
                self.blocks.push(result);
                continue;
            };
            self.blocks.insert(position + 1, result);
        }
    }
}
