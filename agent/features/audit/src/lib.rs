mod adapters;
mod application;
mod domain;
mod ports;

pub use adapters::{file_usage_append_store, FileUsageAppendStore};
pub use application::{
    start_usage_worker, UsagePipelineMetricsSnapshot, UsageSender, UsageShutdownOutcome,
    UsageWorkerConfig, UsageWorkerHandle,
};

pub use domain::{
    Pagination, TimeRange, UsageCursor, UsageDropReason, UsageEmitOutcome, UsageEnvelopeV1,
    UsagePage, UsageQuery, UsageQueryError, UsageQueryWarning, UsageRecord, UsageSummary,
    CURRENT_USAGE_SCHEMA_VERSION,
};
pub use ports::{
    AppendLogError, AppendLogLine, AppendLogNamespace, AppendLogReader, AppendLogStream,
    UsageAppendStorePort, UsageQueryPort,
};

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:audit";
