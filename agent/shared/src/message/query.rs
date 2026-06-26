//! 消息内容查询方法

use crate::message::types::*;

impl Message {
    pub fn extract_tool_uses(&self) -> Vec<(&str, &str, &serde_json::Value)> {
        self.content
            .iter()
            .filter_map(|b| b.as_tool_use())
            .collect()
    }

    pub fn text_content(&self) -> String {
        // 拼接时还原 image block 的 placeholder（`[Image #N]`）到原位
        //（#fix-tui-image-input-output），让上层只读 text 也能看到完整输入。
        self.content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => text.as_str(),
                ContentBlock::Image {
                    placeholder: Some(ph),
                    ..
                } => ph.as_str(),
                _ => "",
            })
            .collect::<String>()
    }

    pub fn source(&self) -> MessageSource {
        self.metadata
            .as_ref()
            .map(|metadata| metadata.source)
            .unwrap_or_default()
    }

    /// Returns true if this message contains any ToolUse blocks.
    pub fn has_tool_uses(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_use())
    }

    /// Returns the ToolUse IDs in this message.
    pub fn tool_use_ids(&self) -> Vec<&str> {
        self.content
            .iter()
            .filter_map(|b| b.tool_use_id())
            .collect()
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
