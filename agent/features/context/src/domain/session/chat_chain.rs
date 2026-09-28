//! Chat 链结构：Session 内按 user 消息分段，compact 产生新链。
//!
//! ## 链语义
//!
//! ```text
//! 正常对话: [Normal(A,null)] → [Normal(B,A)] → [Normal(C,B)]
//! compact 后: [Normal(A,null)] → [Normal(B,A)] → [Normal(C,B)]   ← 旧链冻结
//!                                                                ↘
//!            [Compact(D,null, summary)] → [Normal(E,D)] → [Normal(F,E)]  ← 新链
//! ```
//!
//! resume 只加载活跃链（最后一个 `Compact` 段到末端），天然跳过被压缩的旧历史。

use serde::{Deserialize, Serialize};
use share::ids::ChatId;
use share::message::Message;

/// 段类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    /// 正常对话段（一条 user 消息 + 其触发的完整run，含追问/多轮 tool）
    #[default]
    Normal,
    /// compact 产生的新链起点（`parent_id` 为 None）
    Compact,
}

/// Session 内的一个 chat 段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSegment {
    /// 段 ID（UUIDv7）
    pub id: String,
    /// 父段 ID；Normal 段指向前一段，Compact 段为 None（新链起点）
    #[serde(default)]
    pub parent_id: Option<String>,
    /// 段类型
    #[serde(default)]
    pub kind: SegmentKind,
    /// Compact 段的摘要文本（走 system 通道）；Normal 段为 None
    #[serde(default)]
    pub summary: Option<String>,
    /// 该段的消息列表
    #[serde(default)]
    pub messages: Vec<Message>,
}

impl ChatSegment {
    /// 创建 Normal 段
    pub fn normal(parent_id: Option<String>) -> Self {
        Self {
            id: ChatId::new_v7().to_string(),
            parent_id,
            kind: SegmentKind::Normal,
            summary: None,
            messages: Vec::new(),
        }
    }

    /// 创建 Compact 段（新链起点）
    pub fn compact(summary: String, recent_messages: Vec<Message>) -> Self {
        Self {
            id: ChatId::new_v7().to_string(),
            parent_id: None,
            kind: SegmentKind::Compact,
            summary: Some(summary),
            messages: recent_messages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::message::{ContentBlock, Message, Role};

    fn user_msg(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            metadata: None,
        }
    }

    #[test]
    fn test_segment_kind_default_is_normal() {
        assert_eq!(SegmentKind::default(), SegmentKind::Normal);
    }

    #[test]
    fn test_chat_segment_normal_has_no_summary() {
        let seg = ChatSegment::normal(None);
        assert_eq!(seg.kind, SegmentKind::Normal);
        assert!(seg.parent_id.is_none());
        assert!(seg.summary.is_none());
        assert!(seg.messages.is_empty());
    }

    #[test]
    fn test_chat_segment_compact_carries_summary_and_messages() {
        let msgs = vec![user_msg("recent")];
        let seg = ChatSegment::compact("summary text".to_string(), msgs);
        assert_eq!(seg.kind, SegmentKind::Compact);
        assert!(seg.parent_id.is_none());
        assert_eq!(seg.summary.as_deref(), Some("summary text"));
        assert_eq!(seg.messages.len(), 1);
        assert_eq!(seg.messages[0].text_content(), "recent");
    }

    #[test]
    fn test_serde_roundtrip_normal_segment() {
        let seg = ChatSegment::normal(Some("parent-123".to_string()));
        let json = serde_json::to_string(&seg).unwrap();
        let de: ChatSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(de.kind, SegmentKind::Normal);
        assert_eq!(de.parent_id.as_deref(), Some("parent-123"));
    }

    #[test]
    fn test_serde_roundtrip_compact_segment() {
        let seg = ChatSegment::compact("summary text".to_string(), vec![user_msg("m")]);
        let json = serde_json::to_string(&seg).unwrap();
        let de: ChatSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(de.kind, SegmentKind::Compact);
        assert_eq!(de.summary.as_deref(), Some("summary text"));
        assert_eq!(de.messages.len(), 1);
    }

    #[test]
    fn test_serde_default_missing_fields() {
        // 旧格式 JSON 缺少 parent_id/kind/summary/messages 应能反序列化
        let json = format!("{{\"id\":\"{}\"}}", ChatId::new_v7());
        let de: ChatSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(de.kind, SegmentKind::Normal);
        assert!(de.parent_id.is_none());
        assert!(de.summary.is_none());
        assert!(de.messages.is_empty());
    }

    // ── 新增 API 测试 ──────────────────────────────────
}
