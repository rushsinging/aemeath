mod usage_append_store;

pub use usage_append_store::{
    AppendLogError, AppendLogLine, AppendLogNamespace, AppendLogReader, AppendLogStream,
    UsageAppendStorePort,
};

use async_trait::async_trait;

use crate::{UsagePage, UsageQuery, UsageQueryError, UsageSummary};

#[async_trait]
pub trait UsageQueryPort: Send + Sync {
    async fn query(&self, query: UsageQuery) -> Result<UsagePage, UsageQueryError>;
    async fn summarize(&self, query: UsageQuery) -> Result<UsageSummary, UsageQueryError>;
}
