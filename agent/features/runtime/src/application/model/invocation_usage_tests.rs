use super::record_successful_usage;
use crate::application::loop_engine::chat::InvocationResponse;
use crate::application::model::usage::UsageRecordContext;
use crate::ports::{ModelId, RawUsageSnapshot, UsageSink};
use audit::{UsageDropReason, UsageEmitOutcome, UsageRecord};
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use share::message::Message;
use std::sync::Mutex;

struct RecordingSink {
    outcome: UsageEmitOutcome,
    records: Mutex<Vec<UsageRecord>>,
}

impl RecordingSink {
    fn new(outcome: UsageEmitOutcome) -> Self {
        Self {
            outcome,
            records: Mutex::new(Vec::new()),
        }
    }
}

impl UsageSink for RecordingSink {
    fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome {
        self.records.lock().expect("record lock").push(record);
        self.outcome
    }
}

fn context() -> UsageRecordContext {
    UsageRecordContext {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        run_step_id: RunStepId::new("step"),
        model_invocation_id: ModelInvocationId::new("01900000-0000-7000-8000-000000000004"),
        model: ModelId {
            provider: "provider".to_string(),
            model: "model".to_string(),
        },
    }
}

fn response(usage: RawUsageSnapshot) -> InvocationResponse {
    InvocationResponse {
        assistant_message: Message::user("done"),
        usage,
        stop_reason: provider::ProviderStopReason::EndTurn,
    }
}

#[test]
fn successful_reported_usage_records_once_and_ignores_queue_full() {
    let sink = RecordingSink::new(UsageEmitOutcome::Dropped(UsageDropReason::QueueFull));

    record_successful_usage(
        &sink,
        context(),
        &response(RawUsageSnapshot {
            input_tokens: Some(10),
            output_tokens: Some(2),
            ..RawUsageSnapshot::default()
        }),
        || 99,
    );

    let records = sink.records.lock().expect("record lock");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].model_invocation_id.as_str(),
        "01900000-0000-7000-8000-000000000004"
    );
    assert_eq!(records[0].input_tokens, 10);
}

#[test]
fn successful_unreported_usage_does_not_call_sink() {
    let sink = RecordingSink::new(UsageEmitOutcome::Dropped(
        UsageDropReason::WorkerUnavailable,
    ));

    record_successful_usage(
        &sink,
        context(),
        &response(RawUsageSnapshot::default()),
        || 99,
    );

    assert!(sink.records.lock().expect("record lock").is_empty());
}
