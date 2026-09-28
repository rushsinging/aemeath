//! `share::ids` 内部 id 类型的行为测试（v7 校验、缓存、hash、serde roundtrip）。

use super::ids::*;

#[test]
fn session_and_model_invocation_ids_are_uuidv7_string_contracts() {
    let session = SessionId::new("session-927");
    let invocation = ModelInvocationId::new("invocation-927");

    assert_eq!(session.as_uuid().get_version_num(), 7);
    assert_eq!(invocation.as_uuid().get_version_num(), 7);
    assert!(serde_json::to_value(&session).unwrap().is_string());
    assert!(serde_json::to_value(&invocation).unwrap().is_string());
    assert_eq!(
        serde_json::from_value::<SessionId>(serde_json::to_value(&session).unwrap()).unwrap(),
        session
    );
    assert_eq!(
        serde_json::from_value::<ModelInvocationId>(serde_json::to_value(&invocation).unwrap())
            .unwrap(),
        invocation
    );
}

#[test]
fn test_chat_run_id_new_v7_is_version_7() {
    let id = RunId::new_v7();
    assert_eq!(id.as_uuid().get_version_num(), 7);
    assert_eq!(RunId::parse_uuid7(id.as_str()).unwrap(), id);
}

#[test]
fn test_chat_id_new_v7_is_version_7() {
    let id = ChatId::new_v7();
    assert_eq!(id.as_uuid().get_version_num(), 7);
}

#[test]
fn test_chat_id_parse_uuid7_accepts_v7() {
    let id = ChatId::new_v7();
    let s = id.to_string();
    let parsed = ChatId::parse_uuid7(&s).unwrap();
    assert_eq!(parsed, id);
}

#[test]
fn test_chat_id_parse_uuid7_rejects_v4() {
    // Hardcoded v4 UUID
    let v4 = "550e8400-e29b-41d4-a716-446655440000";
    assert!(ChatId::parse_uuid7(v4).is_err());
}

#[test]
fn test_chat_id_parse_uuid7_rejects_invalid() {
    assert!(ChatId::parse_uuid7("not-a-uuid").is_err());
    assert!(ChatId::parse_uuid7("").is_err());
}

#[test]
fn test_chat_id_from_legacy_or_new_generates_new_for_invalid() {
    let id = ChatId::from_legacy_or_new("chat-1");
    assert_eq!(id.as_uuid().get_version_num(), 7);
}

#[test]
fn test_chat_id_from_legacy_or_new_preserves_v7() {
    let original = ChatId::new_v7();
    let s = original.to_string();
    let restored = ChatId::from_legacy_or_new(&s);
    assert_eq!(restored, original);
}

#[test]
fn test_run_id_new_v7_is_version_7() {
    let id = ChatRunId::new_v7();
    assert_eq!(id.as_uuid().get_version_num(), 7);
}

#[test]
fn test_tool_call_id_new_v7_is_version_7() {
    let id = ToolCallId::new_v7();
    assert_eq!(id.as_uuid().get_version_num(), 7);
}

#[test]
fn test_tool_call_id_parse_rejects_tool_1() {
    assert!(ToolCallId::parse_uuid7("tool-1").is_err());
}

// ---- New tests for the cached String field & AsRef<str> ----

#[test]
fn test_chat_id_as_str_is_borrowed_no_allocation() {
    let id = ChatId::new_v7();
    let s1: &str = id.as_str();
    let s2: &str = id.as_str();
    // Borrowed from the same backing String — pointer equality.
    assert_eq!(s1.as_ptr(), s2.as_ptr());
    assert_eq!(s1, id.to_string().as_str());
}

#[test]
fn test_chat_id_as_ref_str_returns_cached_value() {
    let id = ChatId::new_v7();
    let expected = id.as_uuid().to_string();
    assert_eq!(id.as_ref(), expected.as_str());
    // And a second call returns the same backing buffer.
    assert_eq!(id.as_ref().as_ptr(), id.as_str().as_ptr());
}

#[test]
fn test_chat_id_from_legacy_or_new_caches_uuid_string() {
    let id = ChatId::from_legacy_or_new("chat-1");
    let uuid_str = id.as_uuid().to_string();
    assert_eq!(id.as_str(), uuid_str);
}

#[test]
fn test_chat_id_equality_ignores_cache_difference() {
    // Two ChatIds built from the same UUID string must compare equal.
    // The cached String buffer may live at a different address but
    // identity only depends on the UUID.
    let a = ChatId::from_legacy_or_new("01900000-0000-7000-8000-000000000000");
    let b = ChatId::from_legacy_or_new("01900000-0000-7000-8000-000000000000");
    assert_ne!(a.as_str().as_ptr(), b.as_str().as_ptr());
    assert_eq!(a, b);
}

#[test]
fn test_chat_id_hash_depends_only_on_uuid() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let a = ChatId::new_v7();
    let b = a.clone();
    let mut ha = DefaultHasher::new();
    let mut hb = DefaultHasher::new();
    a.hash(&mut ha);
    b.hash(&mut hb);
    assert_eq!(ha.finish(), hb.finish());
}

#[test]
fn test_chat_id_serde_roundtrip_preserves_uuid() {
    let original = ChatId::new_v7();
    let json = serde_json::to_string(&original).unwrap();
    // Wire format must remain a single JSON string.
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_string(), "expected string, got: {parsed}");
    let restored: ChatId = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, original);
}

#[test]
fn test_tool_call_id_serde_roundtrip_preserves_uuid() {
    let original = ToolCallId::new_v7();
    let json = serde_json::to_string(&original).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_string(), "expected string, got: {parsed}");
    let restored: ToolCallId = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, original);
}

#[test]
fn test_chat_run_id_serde_roundtrip_preserves_uuid() {
    let original = ChatRunId::new_v7();
    let json = serde_json::to_string(&original).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_string(), "expected string, got: {parsed}");
    let restored: ChatRunId = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, original);
}

#[test]
fn test_input_id_new_v7_is_version_7() {
    let id = InputId::new_v7();
    assert_eq!(id.as_uuid().get_version_num(), 7);
}

#[test]
fn test_input_id_serde_roundtrip_preserves_uuid() {
    let original = InputId::new_v7();
    let json = serde_json::to_string(&original).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_string(), "expected string, got: {parsed}");
    let restored: InputId = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, original);
}
