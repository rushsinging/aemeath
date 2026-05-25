//! 消息核心类型定义
//!
//! 定义 Role, ContentBlock, ImageSource, Message 及其相关类型。

use serde::{Deserialize, Serialize};

/// Describes a message integrity issue found during session validation.
#[derive(Debug, Clone, PartialEq)]
pub enum IntegrityIssue {
    /// ToolResult referencing a non-existent ToolUse (e.g., lost during compaction).
    OrphanedToolResult {
        msg_index: usize,
        tool_use_ids: Vec<String>,
    },
    /// Assistant message with tool_calls whose results are missing (not followed
    /// by matching user/ToolResult messages) and those results cannot
    /// be recovered from later messages.
    OrphanedToolUse {
        msg_index: usize,
        tool_ids: Vec<String>,
    },
    /// Back-to-back messages with the same role (user→user or assistant→assistant).
    RoleOrder { msg_index: usize, role: String },
}

/// Results of a message integrity check.
#[derive(Debug, Clone, Default)]
pub struct IntegrityCheck {
    pub issues: Vec<IntegrityIssue>,
}

impl IntegrityCheck {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    /// 获取相反的角色
    pub fn opposite(&self) -> Self {
        match self {
            Role::User => Role::Assistant,
            Role::Assistant => Role::User,
        }
    }
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
    Thinking {
        #[serde(default)]
        thinking: String,
    },
}

impl ContentBlock {
    /// Returns tool_use info if this is a ToolUse block.
    pub fn as_tool_use(&self) -> Option<(&str, &str, &serde_json::Value)> {
        match self {
            ContentBlock::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
            _ => None,
        }
    }

    /// Returns tool_use_id if this is a ToolUse block.
    pub fn tool_use_id(&self) -> Option<&str> {
        match self {
            ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
            _ => None,
        }
    }

    /// Returns tool_use_id if this is a ToolResult block.
    pub fn tool_result_id(&self) -> Option<&str> {
        match self {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        }
    }

    /// Returns true if this is a ToolUse block.
    pub fn is_tool_use(&self) -> bool {
        matches!(self, ContentBlock::ToolUse { .. })
    }

    /// Returns true if this is a ToolResult block.
    pub fn is_tool_result(&self) -> bool {
        matches!(self, ContentBlock::ToolResult { .. })
    }

    /// Create a base64 image ContentBlock.
    pub fn base64_image(data: String, media_type: String) -> Self {
        ContentBlock::Image {
            source: ImageSource::Base64 { media_type, data },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
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
    /// Create a placeholder "(continued)" message of the given role.
    pub fn placeholder(role: Role) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text {
                text: "(continued)".to_string(),
            }],
        }
    }
}
