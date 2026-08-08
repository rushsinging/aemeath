use audit::UsageRecord;
use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};

use crate::ports::{ModelId, RawUsageSnapshot};

#[derive(Clone)]
pub(crate) struct UsageRecordContext {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub run_step_id: RunStepId,
    pub model_invocation_id: ModelInvocationId,
    pub model: ModelId,
}

pub(crate) struct UsageRecordFactory<Clock> {
    clock: Clock,
}

impl<Clock> UsageRecordFactory<Clock>
where
    Clock: Fn() -> u64,
{
    pub(crate) fn new(clock: Clock) -> Self {
        Self { clock }
    }

    pub(crate) fn from_raw_usage(
        &self,
        context: UsageRecordContext,
        usage: RawUsageSnapshot,
    ) -> Option<UsageRecord> {
        usage.was_reported().then(|| UsageRecord {
            recorded_at_unix_ms: (self.clock)(),
            session_id: context.session_id,
            run_id: context.run_id,
            run_step_id: context.run_step_id,
            model_invocation_id: context.model_invocation_id,
            provider: context.model.provider,
            model: context.model.model,
            input_tokens: u64::from(usage.input_tokens.unwrap_or_default()),
            output_tokens: u64::from(usage.output_tokens.unwrap_or_default()),
            cache_write_tokens: usage.cache_write_tokens.map(u64::from),
            cache_read_tokens: usage.cache_read_tokens.map(u64::from),
            reasoning_tokens: usage.reasoning_tokens.map(u64::from),
        })
    }
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
