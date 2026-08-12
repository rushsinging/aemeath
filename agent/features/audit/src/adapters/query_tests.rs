use std::sync::Arc;

use async_trait::async_trait;

use super::query::usage_query_service;
use crate::{
    AppendLogError, AppendLogNamespace, AppendLogReader, AppendLogStream, Pagination,
    UsageAppendStorePort, UsageQuery, UsageQueryError, UsageQueryPort,
};

#[derive(Clone, Copy)]
enum FailingOperation {
    List,
    Read,
}

struct FailingQueryStore {
    operation: FailingOperation,
}

#[async_trait]
impl UsageAppendStorePort for FailingQueryStore {
    async fn append(&self, _: &AppendLogStream, _: &[u8]) -> Result<(), AppendLogError> {
        Err(AppendLogError::Closed)
    }

    async fn flush(&self, _: &AppendLogStream) -> Result<(), AppendLogError> {
        Err(AppendLogError::Closed)
    }

    async fn read(&self, _: &AppendLogStream) -> Result<AppendLogReader, AppendLogError> {
        match self.operation {
            FailingOperation::Read => Err(AppendLogError::Io),
            FailingOperation::List => Ok(AppendLogReader::new(Vec::new())),
        }
    }

    async fn list_streams(
        &self,
        _: &AppendLogNamespace,
    ) -> Result<Vec<AppendLogStream>, AppendLogError> {
        match self.operation {
            FailingOperation::List => Err(AppendLogError::Io),
            FailingOperation::Read => Ok(vec![AppendLogStream::new("stream-a".to_string())]),
        }
    }
}

fn query() -> UsageQuery {
    UsageQuery {
        session_id: None,
        run_id: None,
        run_step_id: None,
        model_invocation_id: None,
        provider: None,
        model: None,
        recorded_range: None,
        pagination: Pagination {
            cursor: None,
            limit: std::num::NonZeroUsize::new(10).expect("non-zero query limit"),
        },
    }
}

#[tokio::test]
async fn query_maps_list_and_read_failures_to_storage_error() {
    for operation in [FailingOperation::List, FailingOperation::Read] {
        let service = usage_query_service(Arc::new(FailingQueryStore { operation }));

        assert_eq!(
            service.query(query()).await,
            Err(UsageQueryError::Storage("审计用量存储读取失败".to_string()))
        );
    }
}
