use audit::{
    Pagination, TimeRange, UsageCursor, UsageDropReason, UsageEmitOutcome, UsageEnvelopeV1,
    UsageQuery, UsageRecord, CURRENT_USAGE_SCHEMA_VERSION,
};
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};

fn fixed_record() -> UsageRecord {
    UsageRecord {
        recorded_at_unix_ms: 1_720_000_000_000,
        session_id: SessionId::new("session-927"),
        run_id: RunId::new("run-927"),
        run_step_id: RunStepId::new("step-927"),
        model_invocation_id: ModelInvocationId::new("invocation-927"),
        provider: "anthropic".to_string(),
        model: "claude-sonnet".to_string(),
        input_tokens: u64::from(u32::MAX) + 1,
        output_tokens: 45,
        cache_write_tokens: Some(7),
        cache_read_tokens: None,
        reasoning_tokens: Some(9),
    }
}

#[test]
fn usage_envelope_v1_serializes_stable_golden_without_sensitive_fields() {
    let envelope = UsageEnvelopeV1::new(fixed_record());
    let json = serde_json::to_string(&envelope).expect("V1 envelope should serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("golden should be JSON");

    assert_eq!(value["schema_version"], 1);
    assert_eq!(
        value["record"]["recorded_at_unix_ms"],
        1_720_000_000_000_u64
    );
    assert_eq!(value["record"]["input_tokens"], u64::from(u32::MAX) + 1);
    assert_eq!(
        value["record"]["cache_read_tokens"],
        serde_json::Value::Null
    );
    assert_eq!(value["record"]["provider"], "anthropic");
    assert_eq!(value["record"]["model"], "claude-sonnet");

    let object = value["record"]
        .as_object()
        .expect("record should be an object");
    for forbidden in [
        "cost",
        "price",
        "prompt",
        "response",
        "thinking",
        "tool_input",
        "tool_output",
        "hook_stdout",
        "hook_stderr",
    ] {
        assert!(
            !object.contains_key(forbidden),
            "forbidden field: {forbidden}"
        );
    }
}

#[test]
fn usage_envelope_v1_round_trip_preserves_record_and_ignores_unknown_fields() {
    let envelope = UsageEnvelopeV1::new(fixed_record());
    let mut value = serde_json::to_value(&envelope).expect("V1 envelope should serialize");
    value["future_optional_field"] = serde_json::json!(true);

    let decoded: UsageEnvelopeV1 =
        serde_json::from_value(value).expect("unknown envelope fields should be compatible");

    assert_eq!(decoded.schema_version, CURRENT_USAGE_SCHEMA_VERSION);
    assert_eq!(decoded.record, envelope.record);
}

#[test]
fn usage_emit_outcome_preserves_structured_drop_reason() {
    assert_eq!(
        UsageEmitOutcome::Dropped(UsageDropReason::QueueFull),
        UsageEmitOutcome::Dropped(UsageDropReason::QueueFull)
    );
    assert_ne!(
        UsageEmitOutcome::Dropped(UsageDropReason::QueueFull),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}

#[test]
fn usage_query_contract_carries_all_correlation_filters() {
    let query = UsageQuery {
        session_id: Some(SessionId::new("session-927")),
        run_id: Some(RunId::new("run-927")),
        run_step_id: Some(RunStepId::new("step-927")),
        model_invocation_id: Some(ModelInvocationId::new("invocation-927")),
        provider: Some("anthropic".to_string()),
        model: Some("claude-sonnet".to_string()),
        recorded_range: Some(TimeRange {
            from_inclusive_unix_ms: Some(10),
            to_exclusive_unix_ms: Some(20),
        }),
        pagination: Pagination {
            cursor: Some(UsageCursor::new("opaque-cursor")),
            limit: std::num::NonZeroUsize::new(50).expect("non-zero"),
        },
    };

    let json = serde_json::to_value(query).expect("query PL should serialize");
    assert!(json.get("session_id").is_some());
    assert!(json.get("run_id").is_some());
    assert!(json.get("run_step_id").is_some());
    assert!(json.get("model_invocation_id").is_some());
}
