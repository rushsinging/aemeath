//! 消息内容查询方法

use crate::message::types::*;

impl Message {
    pub fn extract_tool_uses(&self) -> Vec<(&str, &str, &serde_json::Value)> {
        self.content.iter().filter_map(|b| b.as_tool_use()).collect()
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Returns true if this message contains any ToolUse blocks.
    pub fn has_tool_uses(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_use())
    }

    /// Returns the ToolUse IDs in this message.
    pub fn tool_use_ids(&self) -> Vec<&str> {
        self.content.iter().filter_map(|b| b.tool_use_id()).collect()
    }

    /// Returns true if this message contains ToolResult blocks.
    pub fn has_tool_results(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_result())
    }

    /// Returns the tool_use_ids of ToolResult blocks in this message.
    pub fn tool_result_ids(&self) -> Vec<&str> {
        self.content
            .iter()
            .filter_map(|b| b.tool_result_id())
            .collect()
    }
}
