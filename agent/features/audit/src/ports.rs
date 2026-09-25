mod usage_append_store;

pub use usage_append_store::{
    AppendLogError, AppendLogLine, AppendLogNamespace, AppendLogReader, AppendLogStream,
    UsageAppendStorePort,
};

use async_trait::async_trait;

use crate::domain::{UsagePageData, UsageQueryData, UsageQueryError, UsageSummaryData};

#[async_trait]
#[cfg_attr(not(test), allow(dead_code))]
pub trait UsageQueryPort: Send + Sync {
    async fn query(&self, query: UsageQueryData) -> Result<UsagePageData, UsageQueryError>;
    async fn summarize(&self, query: UsageQueryData) -> Result<UsageSummaryData, UsageQueryError>;
}
