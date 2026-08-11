use super::{UnavailableUsageSink, UsageSink};
use audit::{UsageDropReason, UsageEmitOutcome, UsageRecord};
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use std::sync::Arc;

fn record() -> UsageRecord {
    UsageRecord {
        recorded_at_unix_ms: 1_720_000_000_000,
        session_id: SessionId::new("01900000-0000-7000-8000-000000000001"),
        run_id: RunId::new("01900000-0000-7000-8000-000000000002"),
        run_step_id: RunStepId::new("01900000-0000-7000-8000-000000000003"),
        model_invocation_id: ModelInvocationId::new("01900000-0000-7000-8000-000000000004"),
        provider: "test-provider".to_string(),
        model: "test-model".to_string(),
        input_tokens: 12,
        output_tokens: 3,
        cache_write_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens: None,
    }
}

#[test]
fn unavailable_sink_synchronously_reports_worker_unavailable() {
    let outcome = UnavailableUsageSink.try_record(record());

    assert_eq!(
        outcome,
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}

#[test]
fn usage_sink_trait_object_is_send_sync_and_arc_shareable() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    assert_send_sync::<dyn UsageSink>();
    let sink: Arc<dyn UsageSink> = Arc::new(UnavailableUsageSink);
    let shared = Arc::clone(&sink);

    assert_eq!(
        shared.try_record(record()),
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    );
}
