/// Audit 模块自身的运行诊断 target；Audit Usage Fact 使用独立 append store。
pub(crate) const LOG_TARGET: &str = "aemeath:diagnostic:audit";

mod adapters;
mod application;
mod domain;
mod ports;

pub use adapters::{
    file_usage_append_store, usage_query_service, FileUsageAppendStore, UsageQueryService,
};
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
