use std::num::NonZeroUsize;

use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};

use super::query::{
    add_summary, decode_cursor, decode_record, encode_cursor, matches, query_fingerprint,
    validate_query, CursorPosition, MAX_USAGE_QUERY_LIMIT,
};
use crate::{
    Pagination, TimeRange, UsageEnvelopeV1, UsageQuery, UsageQueryError, UsageQueryWarning,
    UsageRecord, UsageSummary, CURRENT_USAGE_SCHEMA_VERSION,
};

fn record(timestamp: u64) -> UsageRecord {
    UsageRecord {
        recorded_at_unix_ms: timestamp,
        session_id: SessionId::new("session-a"),
        run_id: RunId::new("run-a"),
        run_step_id: RunStepId::new("step-a"),
        model_invocation_id: ModelInvocationId::new("01900000-0000-7000-8000-000000000004"),
        provider: "anthropic".to_string(),
        model: "claude-sonnet".to_string(),
        input_tokens: 10,
        output_tokens: 20,
        cache_write_tokens: Some(3),
        cache_read_tokens: None,
        reasoning_tokens: Some(5),
    }
}

fn query(limit: usize) -> UsageQuery {
    UsageQuery {
        session_id: None,
        run_id: None,
        run_step_id: None,
        model_invocation_id: None,
        provider: None,
        model: None,
        recorded_range: None,
        pagination: Pagination {
            cursor: None,
            limit: NonZeroUsize::new(limit).expect("non-zero query limit"),
        },
    }
}

#[test]
fn validate_query_accepts_half_open_range_and_clamps_limit() {
    let mut request = query(MAX_USAGE_QUERY_LIMIT + 1);
    request.recorded_range = Some(TimeRange {
        from_inclusive_unix_ms: Some(10),
        to_exclusive_unix_ms: Some(11),
    });

    assert_eq!(validate_query(&request), Ok(MAX_USAGE_QUERY_LIMIT));
}

#[test]
fn validate_query_rejects_equal_or_reversed_range() {
    for (from, to) in [(10, 10), (11, 10)] {
        let mut request = query(1);
        request.recorded_range = Some(TimeRange {
            from_inclusive_unix_ms: Some(from),
            to_exclusive_unix_ms: Some(to),
        });

        assert_eq!(
            validate_query(&request),
            Err(UsageQueryError::InvalidRange),
            "range [{from}, {to}) must be rejected"
        );
    }
}

#[test]
fn cursor_round_trip_preserves_unicode_stream_and_fingerprint() {
    let position = CursorPosition {
        stream: "会话-a".to_string(),
        next_line_offset: 42,
        query_fingerprint: "模型=sonnet".to_string(),
    };

    assert_eq!(decode_cursor(&encode_cursor(&position)), Ok(position));
}

#[test]
fn decode_cursor_rejects_bad_version_hex_offset_and_empty_stream() {
    for value in [
        "v2:61:62:1",
        "v1:not-hex:62:1",
        "v1:61:not-hex:1",
        "v1:61:62:not-a-number",
        "v1:61::1",
    ] {
        assert_eq!(
            decode_cursor(value),
            Err(UsageQueryError::InvalidCursor),
            "cursor {value:?} must be rejected"
        );
    }
}

#[test]
fn query_fingerprint_changes_for_each_filter_but_not_pagination() {
    let baseline = query(1);
    let baseline_fingerprint = query_fingerprint(&baseline);

    let mut pagination_only = baseline.clone();
    pagination_only.pagination.limit = NonZeroUsize::new(99).expect("non-zero query limit");
    pagination_only.pagination.cursor = Some(crate::UsageCursor::new("ignored"));
    assert_eq!(query_fingerprint(&pagination_only), baseline_fingerprint);

    let variants = [
        {
            let mut value = baseline.clone();
            value.session_id = Some(SessionId::new("session-b"));
            value
        },
        {
            let mut value = baseline.clone();
            value.run_id = Some(RunId::new("run-b"));
            value
        },
        {
            let mut value = baseline.clone();
            value.run_step_id = Some(RunStepId::new("step-b"));
            value
        },
        {
            let mut value = baseline.clone();
            value.model_invocation_id = Some(ModelInvocationId::new(
                "01900000-0000-7000-8000-000000000005",
            ));
            value
        },
        {
            let mut value = baseline.clone();
            value.provider = Some("openai".to_string());
            value
        },
        {
            let mut value = baseline.clone();
            value.model = Some("gpt".to_string());
            value
        },
        {
            let mut value = baseline;
            value.recorded_range = Some(TimeRange {
                from_inclusive_unix_ms: Some(1),
                to_exclusive_unix_ms: Some(2),
            });
            value
        },
    ];

    for variant in variants {
        assert_ne!(query_fingerprint(&variant), baseline_fingerprint);
    }
}

#[test]
fn decode_record_rejects_unterminated_corrupt_and_unknown_schema_lines() {
    let expected_warning = UsageQueryWarning::CorruptLine {
        stream: "session-a".to_string(),
        line_number: 7,
    };
    let valid = serde_json::to_vec(&UsageEnvelopeV1::new(record(10))).expect("encode record");
    assert_eq!(
        decode_record(&valid, false, "session-a", 7),
        Err(expected_warning.clone())
    );
    assert_eq!(
        decode_record(b"not-json", true, "session-a", 7),
        Err(expected_warning.clone())
    );

    let mut unknown = UsageEnvelopeV1::new(record(10));
    unknown.schema_version = CURRENT_USAGE_SCHEMA_VERSION + 1;
    let unknown = serde_json::to_vec(&unknown).expect("encode unknown schema envelope");
    assert_eq!(
        decode_record(&unknown, true, "session-a", 7),
        Err(expected_warning)
    );
}

#[test]
fn matches_uses_inclusive_start_and_exclusive_end_for_every_filter() {
    let target = record(10);
    let mut request = query(1);
    request.session_id = Some(target.session_id.clone());
    request.run_id = Some(target.run_id.clone());
    request.run_step_id = Some(target.run_step_id.clone());
    request.model_invocation_id = Some(target.model_invocation_id.clone());
    request.provider = Some(target.provider.clone());
    request.model = Some(target.model.clone());
    request.recorded_range = Some(TimeRange {
        from_inclusive_unix_ms: Some(10),
        to_exclusive_unix_ms: Some(11),
    });
    assert!(matches(&request, &target));

    request.recorded_range = Some(TimeRange {
        from_inclusive_unix_ms: Some(9),
        to_exclusive_unix_ms: Some(10),
    });
    assert!(!matches(&request, &target));

    let mismatches = [
        {
            let mut value = query(1);
            value.session_id = Some(SessionId::new("other"));
            value
        },
        {
            let mut value = query(1);
            value.run_id = Some(RunId::new("other"));
            value
        },
        {
            let mut value = query(1);
            value.run_step_id = Some(RunStepId::new("other"));
            value
        },
        {
            let mut value = query(1);
            value.model_invocation_id = Some(ModelInvocationId::new("other"));
            value
        },
        {
            let mut value = query(1);
            value.provider = Some("other".to_string());
            value
        },
        {
            let mut value = query(1);
            value.model = Some("other".to_string());
            value
        },
    ];
    for mismatch in mismatches {
        assert!(!matches(&mismatch, &target));
    }
}

#[test]
fn add_summary_accumulates_optional_tokens_without_cost_fields() {
    let mut summary = UsageSummary::default();
    add_summary(&mut summary, &record(10));
    let mut no_optional_tokens = record(11);
    no_optional_tokens.input_tokens = 7;
    no_optional_tokens.output_tokens = 9;
    no_optional_tokens.cache_write_tokens = None;
    no_optional_tokens.cache_read_tokens = None;
    no_optional_tokens.reasoning_tokens = None;
    add_summary(&mut summary, &no_optional_tokens);

    assert_eq!(summary.record_count, 2);
    assert_eq!(summary.input_tokens, 17);
    assert_eq!(summary.output_tokens, 29);
    assert_eq!(summary.cache_write_tokens, 3);
    assert_eq!(summary.cache_read_tokens, 0);
    assert_eq!(summary.reasoning_tokens, 5);
    let serialized = serde_json::to_value(summary).expect("serialize summary");
    assert!(serialized.get("cost").is_none());
    assert!(serialized.get("price").is_none());
}
