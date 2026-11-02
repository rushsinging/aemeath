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
fn score_report_schema_is_stable() {
    let report = super::summarize(&[super::CaseResult {
        case_id: "c1".to_string(),
        message_count: 1,
        estimated_tokens: 1,
        must_keep_total: 1,
        recall: Some(1.0),
        missed: Vec::new(),
        compression_ratio: Some(0.1),
        summary_chars: Some(1),
        skipped_reason: None,
        degraded: Some(false),
        repair_attempts: 0,
        quality: Some("Llm".to_string()),
    }]);
    let value = serde_json::to_value(&report).expect("score json");
    assert_has_keys(
        &value,
        &[
            "total",
            "evaluated",
            "skipped",
            "must_keep_recall_mean",
            "compression_ratio_mean",
            "degraded_cases",
            "repair_attempts_total",
            "quality",
            "summary_source",
            "cases",
        ],
    );
    assert_has_keys(
        &value["cases"][0],
        &[
            "case_id",
            "message_count",
            "estimated_tokens",
            "must_keep_total",
            "recall",
            "missed",
            "compression_ratio",
            "summary_chars",
            "skipped_reason",
            "degraded",
            "repair_attempts",
            "quality",
        ],
    );
}
