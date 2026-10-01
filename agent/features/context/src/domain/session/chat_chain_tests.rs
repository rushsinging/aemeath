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
