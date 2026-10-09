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

// ── 前缀 typed id（wanaka 方向，#252 先接入 BackgroundProcessId） ────────

#[test]
fn typed_id_generates_prefix_separator_and_base62_snowflake() {
    let value = crate::ids::new_typed_id("task");
    assert!(value.starts_with("task_"), "前缀+下划线：{value}");
    let suffix = &value["task_".len()..];
    assert_eq!(suffix.len(), 11, "64bit 雪花 base62 定长 11 字符：{value}");
    assert!(
        suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()),
        "后缀为 base62：{value}"
    );
    assert!(
        crate::ids::is_typed_id(&value, "task"),
        "自生成 id 校验通过"
    );
}

#[test]
fn typed_id_rejects_wrong_prefix_and_malformed_suffix() {
    let value = crate::ids::new_typed_id("task");
    assert!(
        !crate::ids::is_typed_id(&value, "run"),
        "前缀不匹配必须拒绝"
    );
    assert!(
        !crate::ids::is_typed_id("task_xyz", "task"),
        "非 hex 后缀拒绝"
    );
    assert!(!crate::ids::is_typed_id("task_", "task"), "空后缀拒绝");
    assert!(!crate::ids::is_typed_id("task", "task"), "缺分隔符拒绝");
}

#[test]
fn typed_ids_are_lexicographically_chronological() {
    let early = crate::ids::new_typed_id("task");
    std::thread::sleep(std::time::Duration::from_millis(2));
    let late = crate::ids::new_typed_id("task");
    assert!(early < late, "同前缀字典序=时间序（uuidv7 高位时间戳）");
}

#[test]
fn background_process_id_is_prefixed_form_with_typed_parse() {
    use crate::ids::BackgroundProcessId;
    let process_id = BackgroundProcessId::new_v7();
    let text = process_id.as_str();
    assert!(text.starts_with("process_"), "本体即前缀形态：{text}");
    assert!(crate::ids::is_typed_id(text, "process"));

    let parsed = BackgroundProcessId::parse(text).expect("合法形态可解析");
    assert_eq!(parsed, process_id);

    // 读旧：Background Process 更名前的 `task_` 旧前缀快照仍可解析
    // （历史账本 resume 兼容；新写入一律 `process_`）。
    let legacy_text = format!("task_{}", &text["process_".len()..]);
    assert!(
        BackgroundProcessId::parse(&legacy_text).is_ok(),
        "旧前缀 task_ 快照可解析：{legacy_text}"
    );

    // 跨种类/坏形态拒绝。
    assert!(BackgroundProcessId::parse("run_018f3a2b0d0000000000000000000000").is_err());
    assert!(BackgroundProcessId::parse("task-not-a-suffix").is_err());
    assert!(BackgroundProcessId::parse("").is_err());
}

// ── #1884：雪花生成与 base62 编解码 ─────────────────────────────────

#[test]
fn snowflake_ids_are_unique_under_concurrent_generation() {
    let generated: Vec<u64> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    (0..1000)
                        .map(|_| crate::ids::generate_snowflake())
                        .collect::<Vec<u64>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });
    let mut unique = generated.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), generated.len(), "并发生成不得重复");
}

#[test]
fn base62_round_trips_sixty_four_bit_values() {
    for value in [0u64, 1, 61, 62, 4095, u32::MAX as u64, u64::MAX] {
        let encoded = crate::ids::encode_base62_fixed(value);
        assert_eq!(encoded.len(), 11, "定长 11：{encoded}");
        assert_eq!(crate::ids::decode_base62(&encoded), Some(value));
    }
    // 有序 alphabet + 定长 → 字典序 = 数值序。
    assert!(crate::ids::encode_base62_fixed(12345) < crate::ids::encode_base62_fixed(54321));
}
