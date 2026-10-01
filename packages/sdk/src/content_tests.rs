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
    let json = serde_json::json!({ "type": "tool_use", "id": "1", "name": "Bash", "input": {} });
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
