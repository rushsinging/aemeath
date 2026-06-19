//! Session 快照与摘要。

use serde::{Deserialize, Serialize};

/// SDK 级 message 投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ChatMessageMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessageMetadata {
    #[serde(default)]
    pub source: ChatMessageSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageSource {
    #[default]
    User,
    SystemGenerated,
}

impl ChatMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: serde_json::json!([{ "type": "text", "text": text.into() }]),
            metadata: None,
        }
    }

    pub fn system_generated_user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: serde_json::json!([{ "type": "text", "text": text.into() }]),
            metadata: Some(ChatMessageMetadata {
                source: ChatMessageSource::SystemGenerated,
            }),
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: serde_json::json!([{ "type": "text", "text": text.into() }]),
            metadata: None,
        }
    }

    pub fn user_with_images(text: impl Into<String>, images: Vec<crate::ToolResultImage>) -> Self {
        let mut blocks = vec![serde_json::json!({ "type": "text", "text": text.into() })];
        blocks.extend(images.into_iter().map(|image| {
            serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.media_type,
                    "data": image.base64,
                }
            })
        }));
        Self {
            role: "user".to_string(),
            content: serde_json::Value::Array(blocks),
            metadata: None,
        }
    }

    pub fn text_content(&self) -> String {
        self.content
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|block| {
                        // 只取真正的 `text` 块；**不能**对所有块裸抓 `text` 字段——
                        // 否则 tool_result 块（text-first 后带 `text` 字段）会被误当成消息
                        // 文本，导致工具结果被 MessagesSync 回显成蓝色 user 消息。
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            block.get("text").and_then(|text| text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    }

    pub fn source(&self) -> ChatMessageSource {
        self.metadata
            .as_ref()
            .map(|metadata| metadata.source)
            .unwrap_or_default()
    }
}

/// Session 快照（cheap clone）。
///
/// 底层 Vec 消息通过 Arc 共享，clone 开销低。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Session ID。
    pub id: String,
    /// 消息列表摘要（消息数量）。
    pub message_count: usize,
    /// 总 token 使用量。
    pub total_tokens: u64,
    /// 完整消息列表（仅在 load_session 时填充，snapshot 为 None）。
    pub messages: Vec<ChatMessage>,
    /// 创建时间（ISO 8601）。
    pub created_at: Option<String>,
    /// 消息清洗中移除的消息数。
    pub trimmed: usize,
    /// 消息清洗中修复的消息数。
    pub repaired: usize,
    /// 会话 workspace 上下文（若存在）。
    pub workspace: Option<crate::WorkspaceContextView>,
    /// 任务快照（Session 恢复时用于重建任务状态）。
    pub tasks: Option<serde_json::Value>,
}

/// Session 列表中的摘要条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Session ID。
    pub id: String,
    /// 用户自定义标题。
    pub title: Option<String>,
    /// 所属项目。
    pub project: Option<String>,
    /// 使用模型。
    pub model: Option<String>,
    /// 创建时间。
    pub created_at: String,
    /// 最后更新时间。
    pub updated_at: String,
    /// 消息数量。
    pub message_count: usize,
    /// 首条用户消息预览。
    pub preview: Option<String>,
    /// 展示摘要。
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::ChatMessage;

    fn msg(content: serde_json::Value) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content,
            metadata: None,
        }
    }

    #[test]
    fn test_text_content_extracts_text_blocks() {
        // 正常路径：text 块的文本被提取。
        let m = msg(serde_json::json!([{ "type": "text", "text": "hello" }]));
        assert_eq!(m.text_content(), "hello");
    }

    #[test]
    fn test_text_content_ignores_tool_result_text_field() {
        // 回归：tool_result 块（text-first 后带 `text` 字段）不应被当成消息文本，
        // 否则工具结果会被 MessagesSync 回显成 user 消息。
        let m = msg(serde_json::json!([{
            "type": "tool_result",
            "tool_use_id": "t1",
            "content": { "stdout": "lots of output" },
            "is_error": false,
            "text": "lots of output"
        }]));
        assert_eq!(
            m.text_content(),
            "",
            "tool_result 的 text 字段不应进入 text_content"
        );
    }

    #[test]
    fn test_text_content_mixed_only_text_blocks() {
        // 边界：text + tool_result 混合 → 只取 text 块。
        let m = msg(serde_json::json!([
            { "type": "text", "text": "answer" },
            { "type": "tool_result", "tool_use_id": "t1", "content": {}, "is_error": false, "text": "tool out" }
        ]));
        assert_eq!(m.text_content(), "answer");
    }

    #[test]
    fn test_text_content_non_array_content() {
        // 错误/边界路径：content 非数组 → 空串。
        let m = msg(serde_json::json!("not an array"));
        assert_eq!(m.text_content(), "");
    }
}
