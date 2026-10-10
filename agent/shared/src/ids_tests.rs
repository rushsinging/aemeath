//! `share::ids` typed id 行为测试（生成形状、legacy 兼容、serde、雪花）。

use super::ids::*;

#[test]
fn new_v7_ids_use_registered_typed_prefixes() {
    assert!(is_typed_id(ChatId::new_v7().as_str(), "cht"));
    assert!(is_typed_id(ChatRunId::new_v7().as_str(), "run"));
    assert!(is_typed_id(RunId::new_v7().as_str(), "run"));
    assert!(is_typed_id(SessionId::new_v7().as_str(), "ses"));
    assert!(is_typed_id(RunStepId::new_v7().as_str(), "stp"));
    assert!(is_typed_id(ModelInvocationId::new_v7().as_str(), "inv"));
    assert!(is_typed_id(AgentId::new_v7().as_str(), "agt"));
    assert!(is_typed_id(InteractionRequestId::new_v7().as_str(), "irq"));
    assert!(is_typed_id(ToolCallId::new_v7().as_str(), "tcl"));
    assert!(is_typed_id(InputId::new_v7().as_str(), "inp"));
    assert!(is_typed_id(ActivityId::new_v7().as_str(), "act"));
}

#[test]
fn parse_accepts_typed_and_legacy_uuidv7() {
    let typed = RunId::new_v7();
    assert_eq!(RunId::parse(typed.as_str()).unwrap(), typed);
    assert_eq!(RunId::parse_uuid7(typed.as_str()).unwrap(), typed);

    let legacy = "01234567-89ab-7cde-8f01-234567890abc";
    let parsed = ChatId::parse(legacy).unwrap();
    assert_eq!(parsed.as_str(), legacy);
}

#[test]
fn parse_rejects_uuidv4_and_garbage() {
    let v4 = "550e8400-e29b-41d4-a716-446655440000";
    assert!(matches!(
        ChatId::parse(v4),
        Err(IdParseError::NotVersion7(_))
    ));
    assert!(ChatId::parse("not-a-uuid").is_err());
    assert!(ChatId::parse("").is_err());
    assert!(ChatId::parse("run_07XDnXyhBz6").is_err()); // 错误前缀
}

#[test]
fn from_legacy_or_new_maps_invalid_strings_deterministically() {
    let a = ChatId::from_legacy_or_new("chat-1");
    let b = ChatId::new("chat-1");
    assert_eq!(a, b);
    assert!(uuid::Uuid::parse_str(a.as_str()).unwrap().get_version_num() == 7);
    assert_ne!(a.as_str(), "chat-1");
}

#[test]
fn session_and_model_invocation_serde_round_trip() {
    let session = SessionId::new_v7();
    let invocation = ModelInvocationId::new_v7();

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
fn legacy_uuidv7_serde_round_trip() {
    let legacy = "01234567-89ab-7cde-8f01-234567890abc";
    let id = ToolCallId::parse(legacy).unwrap();
    let json = serde_json::to_value(&id).unwrap();
    assert_eq!(json, serde_json::json!(legacy));
    assert_eq!(serde_json::from_value::<ToolCallId>(json).unwrap(), id);
}

#[test]
fn display_and_as_ref_expose_inner_string() {
    let id = InputId::new_v7();
    assert_eq!(id.as_str(), id.as_ref());
    assert_eq!(id.to_string(), id.as_str());
}

#[test]
fn hash_and_eq_use_string_identity() {
    use std::collections::HashSet;
    let id = AgentId::new("agent-x");
    let mut set = HashSet::new();
    set.insert(id.clone());
    assert!(set.contains(&AgentId::new("agent-x")));
    assert!(!set.contains(&AgentId::new("agent-y")));
}

#[test]
fn new_typed_id_emits_prefix_and_fixed_suffix() {
    let value = crate::ids::new_typed_id("task");
    assert!(value.starts_with("task_"), "必须以 type_ 开头: {value}");
    let suffix = &value["task_".len()..];
    assert_eq!(suffix.len(), 11, "雪花 base62 定长 11: {suffix}");
    assert!(
        suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()),
        "后缀须为 base62: {suffix}"
    );
    assert!(crate::ids::is_typed_id(&value, "task"));
}

#[test]
fn typed_id_rejects_wrong_prefix_and_malformed_suffix() {
    let value = crate::ids::new_typed_id("task");
    assert!(!crate::ids::is_typed_id(&value, "run"));
    assert!(!crate::ids::is_typed_id("task_xyz", "task"));
    assert!(!crate::ids::is_typed_id("task_", "task"));
    assert!(!crate::ids::is_typed_id("task", "task"));
}

#[test]
fn typed_ids_are_lexicographically_chronological() {
    let early = crate::ids::new_typed_id("task");
    std::thread::sleep(std::time::Duration::from_millis(2));
    let late = crate::ids::new_typed_id("task");
    assert!(early < late, "同前缀字典序=时间序");
}

#[test]
fn background_process_id_is_prefixed_form_with_typed_parse() {
    let process_id = BackgroundProcessId::new_v7();
    let text = process_id.as_str();
    assert!(text.starts_with("bgp_"), "本体即前缀形态：{text}");
    assert!(crate::ids::is_typed_id(text, "bgp"));

    let parsed = BackgroundProcessId::parse(text).expect("合法形态可解析");
    assert_eq!(parsed, process_id);

    for legacy_prefix in ["process_", "task_"] {
        let legacy_text = format!("{legacy_prefix}{}", &text["bgp_".len()..]);
        assert!(
            BackgroundProcessId::parse(&legacy_text).is_ok(),
            "旧前缀快照可解析：{legacy_text}"
        );
    }

    assert!(BackgroundProcessId::parse("run_07XDnXyhBz6").is_err());
    assert!(BackgroundProcessId::parse("task-not-a-suffix").is_err());
    assert!(BackgroundProcessId::parse("").is_err());
}

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
    assert!(crate::ids::encode_base62_fixed(12345) < crate::ids::encode_base62_fixed(54321));
}
