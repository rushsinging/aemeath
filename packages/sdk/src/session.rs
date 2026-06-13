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
                    .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
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
