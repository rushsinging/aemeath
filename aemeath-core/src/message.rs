use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImageSource {
    Base64 {
        media_type: String,
        data: String,
    },
}

/// Image dimensions for display and coordinate mapping
#[derive(Debug, Clone, Default)]
pub struct ImageDimensions {
    pub original_width: Option<u32>,
    pub original_height: Option<u32>,
    pub display_width: Option<u32>,
    pub display_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn user_with_image(text: impl Into<String>, image_base64: String, media_type: String) -> Self {
        Self {
            role: Role::User,
            content: vec![
                ContentBlock::Image {
                    source: ImageSource::Base64 {
                        media_type,
                        data: image_base64,
                    },
                },
                ContentBlock::Text { text: text.into() },
            ],
        }
    }

    pub fn user_with_images(text: impl Into<String>, images: Vec<(String, String)>) -> Self {
        let mut content: Vec<ContentBlock> = images
            .into_iter()
            .map(|(data, media_type)| ContentBlock::Image {
                source: ImageSource::Base64 { media_type, data },
            })
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
    pub fn tool_results_rich(results: Vec<(String, String, bool, Vec<crate::tool::ImageData>)>) -> Self {
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
                            .map(|img| serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": img.media_type,
                                    "data": img.base64,
                                }
                            }))
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

    pub fn extract_tool_uses(&self) -> Vec<(&str, &str, &serde_json::Value)> {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.as_str(), name.as_str(), input))
                }
                _ => None,
            })
            .collect()
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
}
