//! L3 契约测试：评测产物 JSON schema 稳定性（键存在性，不锁取值）。
//!
//! 这些结构是评测链路的对外契约（`results/` 入库、下游报告消费），
//! 字段改名/删除会静默破坏历史基线对比，因此用必需键集合钉死。

use serde_json::Value;

fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn assert_has_keys(value: &Value, required: &[&str]) {
    let present = keys(value);
    for key in required {
        assert!(
            present.iter().any(|item| item == key),
            "缺少契约字段 {key}；实际字段: {present:?}"
        );
    }
}

#[test]
fn case_json_schema_is_stable() {
    let case = super::Case {
        case_id: "c1".to_string(),
        session_id: "c1".to_string(),
        model: None,
        source_dataset: "/tmp/x.dataset".to_string(),
        committed_step_count: 1,
        message_count: 2,
        estimated_tokens: 3,
        previous_summary: String::new(),
        must_keep: vec!["a.rs".to_string()],
        must_keep_source: "auto_identifiers".to_string(),
        messages_path: "c1/messages.jsonl".to_string(),
        compacted_message_count: 0,
    };
    let value = serde_json::to_value(&case).expect("case json");
    assert_has_keys(
        &value,
        &[
            "case_id",
            "session_id",
            "model",
            "source_dataset",
            "committed_step_count",
            "message_count",
            "estimated_tokens",
            "previous_summary",
            "must_keep",
            "must_keep_source",
            "messages_path",
            "compacted_message_count",
        ],
    );
}
