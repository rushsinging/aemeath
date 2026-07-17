use std::num::NonZeroUsize;

use sdk::{ModelInvocationId, RunId, RunStepId, SessionId};
use serde::{Deserialize, Serialize};

pub const CURRENT_USAGE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub recorded_at_unix_ms: u64,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub run_step_id: RunStepId,
    pub model_invocation_id: ModelInvocationId,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEnvelopeV1 {
    pub schema_version: u32,
    pub record: UsageRecord,
}

impl UsageEnvelopeV1 {
    pub fn new(record: UsageRecord) -> Self {
        Self {
            schema_version: CURRENT_USAGE_SCHEMA_VERSION,
            record,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageEmitOutcome {
    Accepted,
    Dropped(UsageDropReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageDropReason {
    QueueFull,
    WorkerUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageQuery {
    pub session_id: Option<SessionId>,
    pub run_id: Option<RunId>,
    pub run_step_id: Option<RunStepId>,
    pub model_invocation_id: Option<ModelInvocationId>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub recorded_range: Option<TimeRange>,
    pub pagination: Pagination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub from_inclusive_unix_ms: Option<u64>,
    pub to_exclusive_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pagination {
    pub cursor: Option<UsageCursor>,
    pub limit: NonZeroUsize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UsageCursor(String);

impl UsageCursor {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsagePage {
    pub records: Vec<UsageRecord>,
    pub next_cursor: Option<UsageCursor>,
    pub warnings: Vec<UsageQueryWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UsageSummary {
    pub record_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageQueryError {
    Storage(String),
    InvalidRange,
    InvalidCursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageQueryWarning {
    CorruptLine { stream: String, line_number: u64 },
}
