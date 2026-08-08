use super::{UsageRecordContext, UsageRecordFactory};
use crate::ports::{ModelId, RawUsageSnapshot};
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};

fn context() -> UsageRecordContext {
    UsageRecordContext {
        session_id: SessionId::new("01900000-0000-7000-8000-000000000001"),
        run_id: RunId::new("01900000-0000-7000-8000-000000000002"),
        run_step_id: RunStepId::new("01900000-0000-7000-8000-000000000003"),
        model_invocation_id: ModelInvocationId::new("01900000-0000-7000-8000-000000000004"),
        model: ModelId {
            provider: "provider-a".to_string(),
            model: "model-b".to_string(),
        },
    }
}

#[test]
fn factory_maps_reported_usage_and_preserves_correlation() {
    let context = context();
    let expected_context = context.clone();
    let usage = RawUsageSnapshot {
        input_tokens: Some(u32::MAX),
        output_tokens: Some(23),
        cache_write_tokens: Some(7),
        cache_read_tokens: Some(5),
        reasoning_tokens: Some(11),
    };

    let record = UsageRecordFactory::new(|| 1_720_000_000_123)
        .from_raw_usage(context, usage)
        .expect("reported usage must produce a record");

    assert_eq!(record.recorded_at_unix_ms, 1_720_000_000_123);
    assert_eq!(record.session_id, expected_context.session_id);
    assert_eq!(record.run_id, expected_context.run_id);
    assert_eq!(record.run_step_id, expected_context.run_step_id);
    assert_eq!(
        record.model_invocation_id,
        expected_context.model_invocation_id
    );
    assert_eq!(record.provider, "provider-a");
    assert_eq!(record.model, "model-b");
    assert_eq!(record.input_tokens, u64::from(u32::MAX));
    assert_eq!(record.output_tokens, 23);
    assert_eq!(record.cache_write_tokens, Some(7));
    assert_eq!(record.cache_read_tokens, Some(5));
    assert_eq!(record.reasoning_tokens, Some(11));
}

#[test]
fn factory_distinguishes_unreported_usage_from_reported_zero() {
    let factory = UsageRecordFactory::new(|| 42);

    assert!(factory
        .from_raw_usage(context(), RawUsageSnapshot::default())
        .is_none());

    let record = factory
        .from_raw_usage(
            context(),
            RawUsageSnapshot {
                cache_read_tokens: Some(0),
                ..RawUsageSnapshot::default()
            },
        )
        .expect("reported zero must produce a record");
    assert_eq!(record.input_tokens, 0);
    assert_eq!(record.output_tokens, 0);
    assert_eq!(record.cache_write_tokens, None);
    assert_eq!(record.cache_read_tokens, Some(0));
    assert_eq!(record.reasoning_tokens, None);
}
