//! 消息构造方法

use crate::message::types::*;
use serde_json;

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn user_with_image(
        text: impl Into<String>,
        image_base64: String,
        media_type: String,
    ) -> Self {
        Self {
            role: Role::User,
            content: vec![
                ContentBlock::base64_image(image_base64, media_type),
                ContentBlock::Text { text: text.into() },
            ],
        }
    }

    pub fn user_with_images(text: impl Into<String>, images: Vec<(String, String)>) -> Self {
        let mut content: Vec<ContentBlock> = images
            .into_iter()
            .map(|(data, media_type)| ContentBlock::base64_image(data, media_type))
            .collect();
        content.push(ContentBlock::Text { text: text.into() });
        Self {
            role: Role::User,
            content,
        }
    }

    pub fn tool_results(results: Vec<(String, String, bool)>) -> Self {
        Self {
            role: Role::User,
            content: results
                .into_iter()
                .map(|(tool_use_id, content, is_error)| ContentBlock::ToolResult {
                    tool_use_id,
                    content: serde_json::Value::String(content),
                    is_error,
                })
                .collect(),
        }
    }

    /// Create tool results with optional image attachments.
    /// Each result is (tool_use_id, text_content, is_error, images).
    pub fn tool_results_rich(
        results: Vec<(String, String, bool, Vec<crate::tool::ImageData>)>,
    ) -> Self {
        Self {
            role: Role::User,
            content: results
                .into_iter()
                .map(|(tool_use_id, text, is_error, images)| {
                    let content = if images.is_empty() {
                        serde_json::Value::String(text)
                    } else {
                        let mut blocks: Vec<serde_json::Value> = images
                            .into_iter()
                            .map(|img| {
                                serde_json::json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": img.media_type,
                                        "data": img.base64,
                                    }
                                })
                            })
                            .collect();
                        blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                        serde_json::Value::Array(blocks)
                    };
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    }
                })
                .collect(),
        }
    }
}
