//! Session 快照与摘要。

use serde::{Deserialize, Serialize};

/// SDK 级 message 投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
}

impl ChatMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: serde_json::json!([{ "type": "text", "text": text.into() }]),
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: serde_json::json!([{ "type": "text", "text": text.into() }]),
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
