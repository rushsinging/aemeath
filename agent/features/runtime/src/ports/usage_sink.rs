use audit::{UsageDropReason, UsageEmitOutcome, UsageRecord};

/// Runtime-owned non-blocking outbound port for Audit usage facts.
pub trait UsageSink: Send + Sync {
    fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome;
}

/// Default sink used when the Audit worker is not available.
#[derive(Debug, Default)]
pub struct UnavailableUsageSink;

impl UsageSink for UnavailableUsageSink {
    fn try_record(&self, _record: UsageRecord) -> UsageEmitOutcome {
        UsageEmitOutcome::Dropped(UsageDropReason::WorkerUnavailable)
    }
}

#[cfg(test)]
#[path = "usage_sink_tests.rs"]
mod tests;
