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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MessageMetadata {
    #[serde(default)]
    pub source: MessageSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageSource {
    #[default]
    User,
    SystemGenerated,
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
        /// TUI 端输入时的占位符（形如 `"[Image #1]"`），用于 session resume 时把
        /// image block 重新插入到 text 中正确位置（#fix-tui-image-input-output）。
        /// round-trip 字段，**不发给 LLM**——provider adapter 组装时丢弃。
        /// 旧 history 没有此字段，`#[serde(default)]` 兼容。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        /// 结构化结果（TUI / server 边界按 tool_name 的 Output schema 反序列化）。
        /// 持久化忠实保留它；发送给 LLM 时由 `build_api_messages` 降为 text-first。
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
        /// 给 LLM 的 text-first 文本（工具 `ToolOutcome::text`）。
        /// 仅出站到 LLM 时使用；为 `None`（旧 session / 占位符）时回退发送 `content`。
        /// 默认省略，发送给 LLM 的 wire 块会被重建为不含此字段的干净 tool_result。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
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
            placeholder: None,
        }
    }

    /// 返回发送给 LLM 的 **text-first** 视图。
    ///
    /// 对 `ToolResult`：若带 `text`（新工具结果），content 降为该文本、并剥离结构化
    /// `data` 与 `text` 字段，使 LLM 只收到文本（省 token、更友好）；若 `text` 为
    /// `None`（旧 session / compaction 占位符），原样保留 `content` 向后兼容。
    /// 其它块原样克隆。持久化的 `messages` 不受影响（仍忠实保留结构化 `content`）。
    pub fn to_llm_view(&self) -> ContentBlock {
        match self {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
                text,
            } => ContentBlock::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: match text {
                    Some(t) => Self::tool_result_text_first(content, t),
                    None => content.clone(),
                },
                is_error: *is_error,
                text: None,
            },
            other => other.clone(),
        }
    }

    /// 构造 text-first 的 tool_result content：
    /// - content 是多块数组（含图片）：保留 `image` 块 + 一个 `{type:text,text}`，
    ///   丢弃结构化 `json` 块；
    /// - 否则：直接用文本字符串。
    fn tool_result_text_first(content: &serde_json::Value, text: &str) -> serde_json::Value {
        if let serde_json::Value::Array(blocks) = content {
            let mut out: Vec<serde_json::Value> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("image"))
                .cloned()
                .collect();
            out.push(serde_json::json!({ "type": "text", "text": text }));
            serde_json::Value::Array(out)
        } else {
            serde_json::Value::String(text.to_string())
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
}

impl Message {
    /// Create a placeholder "(continued)" message of the given role.
    pub fn placeholder(role: Role) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text {
                text: "(continued)".to_string(),
            }],
            metadata: None,
        }
    }

    /// 返回发送给 LLM 的 text-first 视图（逐块 `ContentBlock::to_llm_view`）。
    /// 持久化的原消息不变。
    pub fn to_llm_view(&self) -> Message {
        Message {
            role: self.role.clone(),
            content: self.content.iter().map(|b| b.to_llm_view()).collect(),
            metadata: self.metadata.clone(),
        }
    }
}

#[cfg(test)]
mod to_llm_view_tests {
    use super::*;

    #[test]
    fn test_tool_result_with_text_becomes_text_first() {
        // 正常路径：带 text → content 降为文本字符串，剥离 data 与 text 字段。
        let block = ContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: serde_json::json!({"stdout": "hello", "exit_code": 0}),
            is_error: false,
            text: Some("hello".into()),
        };
        let view = block.to_llm_view();
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["content"], serde_json::json!("hello"));
        assert!(json.get("text").is_none(), "text 字段应被剥离: {json}");
        assert_eq!(json["is_error"], serde_json::json!(false));
    }

    #[test]
    fn test_tool_result_without_text_keeps_content() {
        // 向后兼容：text=None（旧 session / 占位符）→ 原样保留结构化 content。
        let block = ContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: serde_json::json!({"stdout": "x"}),
            is_error: false,
            text: None,
        };
        let view = block.to_llm_view();
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["content"], serde_json::json!({"stdout": "x"}));
    }

    #[test]
    fn test_tool_result_with_images_keeps_image_blocks_drops_data() {
        // 边界：含图片的多块数组 → 保留 image 块 + 文本块，丢弃 json 数据块。
        let block = ContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: serde_json::json!([
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}},
                {"type": "text", "text": "old"},
                {"type": "json", "json": {"k": "v"}}
            ]),
            is_error: false,
            text: Some("desc".into()),
        };
        let view = block.to_llm_view();
        let json = serde_json::to_value(&view).unwrap();
        let arr = json["content"].as_array().expect("array content");
        assert_eq!(arr.len(), 2, "保留 image + text，丢弃 json: {json}");
        assert_eq!(arr[0]["type"], serde_json::json!("image"));
        assert_eq!(arr[1], serde_json::json!({"type": "text", "text": "desc"}));
    }

    #[test]
    fn test_non_tool_result_block_unchanged() {
        let block = ContentBlock::Text { text: "hi".into() };
        let view = block.to_llm_view();
        assert!(matches!(view, ContentBlock::Text { text } if text == "hi"));
    }
}
