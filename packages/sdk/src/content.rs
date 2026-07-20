//! sdk 级 typed 消息内容块。
//!
//! 与 runtime `share::message::ContentBlock` **同形**，serde 序列化成完全相同的 JSON
//! （持久化 / server 契约不变）。sdk 不依赖 share——这是 CLI↔Runtime 契约层独立定义，
//! 由 `runtime/core/client/mapping.rs` 做 share↔sdk 映射。
//!
//! 引入目的：TUI / server 消费消息时走 typed 分发，**杜绝**到处裸抓
//! `serde_json::Value` 的 `get("text")` / `get("type")`（见 #388 设计、#386 回归）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 消息内容块。`#[serde(tag = "type")]` + snake_case 与 wire / 持久化 JSON 对齐。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
        /// TUI 端输入时的占位符（形如 `"[Image #1]"`），用于 session resume 时
        /// 把 image block 重新插入到 text 中正确位置（#fix-tui-image-input-output）。
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
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
        /// text-first 文本（主重构 Phase B 引入）。仅 round-trip，**不**参与消息文本提取。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
        /// Anthropic extended-thinking 签名（与 share::ContentBlock 同形）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    /// 是否为 Text 块。
    pub fn is_text(&self) -> bool {
        matches!(self, ContentBlock::Text { .. })
    }

    /// 是否为 ToolResult 块。
    pub fn is_tool_result(&self) -> bool {
        matches!(self, ContentBlock::ToolResult { .. })
    }

    /// 是否为 ToolUse 块。
    pub fn is_tool_use(&self) -> bool {
        matches!(self, ContentBlock::ToolUse { .. })
    }

    /// Text 块的文本（其它块返回 None）。
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_block_roundtrip() {
        let b = ContentBlock::text("hi");
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json, serde_json::json!({ "type": "text", "text": "hi" }));
        let back: ContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(back, b);
    }

    #[test]
    fn test_tool_result_deserializes_with_text_field() {
        // 持久化 JSON（Phase B 后带 text 字段）能反序列化为 typed 块。
        let json = serde_json::json!({
            "type": "tool_result",
            "tool_use_id": "t1",
            "content": { "stdout": "out" },
            "is_error": false,
            "text": "out"
        });
        let b: ContentBlock = serde_json::from_value(json).unwrap();
        assert!(b.is_tool_result());
        assert!(b.as_text().is_none(), "tool_result 不是 Text 块");
    }

    #[test]
    fn test_tool_use_classification() {
        let json =
            serde_json::json!({ "type": "tool_use", "id": "1", "name": "Bash", "input": {} });
        let b: ContentBlock = serde_json::from_value(json).unwrap();
        assert!(b.is_tool_use());
        assert!(!b.is_text());
    }

    #[test]
    fn test_thinking_with_signature_roundtrip() {
        let b = ContentBlock::Thinking {
            thinking: "reasoning".to_string(),
            signature: Some("sig_abc".to_string()),
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "thinking", "thinking": "reasoning", "signature": "sig_abc" })
        );
        let back: ContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(back, b);
    }

    #[test]
    fn test_thinking_without_signature_skips_field() {
        let b = ContentBlock::Thinking {
            thinking: "reasoning".to_string(),
            signature: None,
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "thinking", "thinking": "reasoning" })
        );
    }

    #[test]
    fn test_thinking_legacy_json_deserializes_with_default_signature() {
        // 旧 session 的 thinking block 没有 signature 字段
        let json = serde_json::json!({ "type": "thinking", "thinking": "old" });
        let b: ContentBlock = serde_json::from_value(json).unwrap();
        match b {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "old");
                assert_eq!(signature, None);
            }
            _ => panic!("expected Thinking"),
        }
    }
}
