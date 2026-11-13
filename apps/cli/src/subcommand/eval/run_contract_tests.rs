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
fn run_summary_schema_is_stable() {
    let report = super::RunReport {
        cases_total: 1,
        succeeded: 1,
        elapsed_ms_p50: Some(1),
        elapsed_ms_p95: Some(1),
        elapsed_ms_max: Some(1),
        ms_per_1k_input_tokens_mean: Some(1.0),
        degraded_chunks_total: 0,
        cases: vec![super::CaseRun {
            case_id: "c1".to_string(),
            status: "succeeded".to_string(),
            message_count: 1,
            summary_chars: Some(1),
            recent_messages: Some(1),
            quality: Some("Llm".to_string()),
            elapsed_ms: Some(1),
            estimated_input_tokens: Some(1),
            ms_per_1k_input_tokens: Some(1.0),
            metrics: None,
            error: None,
        }],
        ..Default::default()
    };
    let value = serde_json::to_value(&report).expect("run json");
    assert_has_keys(
        &value,
        &[
            "cases_total",
            "succeeded",
            "failed",
            "skipped",
            "binding_model",
            "context_size",
            "elapsed_ms_p50",
            "elapsed_ms_p95",
            "elapsed_ms_max",
            "ms_per_1k_input_tokens_mean",
            "degraded_chunks_total",
            "cases",
        ],
    );
    let case = &value["cases"][0];
    assert_has_keys(
        case,
        &[
            "case_id",
            "status",
            "message_count",
            "summary_chars",
            "recent_messages",
            "quality",
            "elapsed_ms",
            "estimated_input_tokens",
            "ms_per_1k_input_tokens",
            "metrics",
            "error",
        ],
    );
}
