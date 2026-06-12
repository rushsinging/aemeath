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
        if self.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::ToolCall { id: existing, .. } if existing == &id)
        }) {
            return;
        }
        let block = ConversationBlock::ToolCall {
            id,
            name,
            summary,
            args_preview,
        };
        self.blocks.push(block);
    }

    pub(super) fn insert_tool_result_after_tool_call(
        &mut self,
        id: ToolCallId,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    ) {
        self.blocks.retain(|existing| {
            !matches!(existing, ConversationBlock::ToolResult { id: result_id, .. } if result_id == &id)
        });
        let block = ConversationBlock::ToolResult {
            id: id.clone(),
            output,
            content,
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
