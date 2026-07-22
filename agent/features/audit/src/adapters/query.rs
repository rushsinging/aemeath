use std::sync::Arc;

use async_trait::async_trait;

use crate::application::query::{
    add_summary, decode_cursor, decode_record, encode_cursor, matches, query_fingerprint,
    validate_query, CursorPosition,
};
use crate::{
    AppendLogNamespace, AppendLogStream, UsageAppendStorePort, UsageCursor, UsagePage, UsageQuery,
    UsageQueryError, UsageQueryPort, UsageSummary,
};

pub fn usage_query_service(store: Arc<dyn UsageAppendStorePort>) -> UsageQueryService {
    UsageQueryService { store }
}

pub struct UsageQueryService {
    store: Arc<dyn UsageAppendStorePort>,
}

#[async_trait]
impl UsageQueryPort for UsageQueryService {
    async fn query(&self, query: UsageQuery) -> Result<UsagePage, UsageQueryError> {
        let limit = validate_query(&query)?;
        let cursor = query
            .pagination
            .cursor
            .as_ref()
            .map(|cursor| decode_cursor(cursor.as_str()))
            .transpose()?;
        if cursor
            .as_ref()
            .is_some_and(|cursor| cursor.query_fingerprint != query_fingerprint(&query))
        {
            return Err(UsageQueryError::InvalidCursor);
        }
        let streams = self.streams(&query, cursor.as_ref()).await?;
        let mut records = Vec::with_capacity(limit);
        let mut warnings = Vec::new();

        for stream in streams {
            let start = cursor
                .as_ref()
                .filter(|cursor| cursor.stream == stream.as_str())
                .map_or(0, |cursor| cursor.next_line_offset);
            let reader = self.store.read(&stream).await.map_err(storage_error)?;
            for (offset, line) in reader.lines().iter().enumerate().skip(start) {
                let line_number = u64::try_from(offset + 1).unwrap_or(u64::MAX);
                match decode_record(
                    line.bytes(),
                    line.is_terminated(),
                    stream.as_str(),
                    line_number,
                ) {
                    Ok(record) if matches(&query, &record) => {
                        if records.len() == limit {
                            return Ok(UsagePage {
                                records,
                                next_cursor: Some(UsageCursor::new(encode_cursor(
                                    &CursorPosition {
                                        stream: stream.as_str().to_string(),
                                        next_line_offset: offset,
                                        query_fingerprint: query_fingerprint(&query),
                                    },
                                ))),
                                warnings,
                            });
                        }
                        records.push(record);
                    }
                    Ok(_) => {}
                    Err(warning) => warnings.push(warning),
                }
            }
        }

        Ok(UsagePage {
            records,
            next_cursor: None,
            warnings,
        })
    }

    async fn summarize(&self, query: UsageQuery) -> Result<UsageSummary, UsageQueryError> {
        validate_query(&query)?;
        if query.pagination.cursor.is_some() {
            return Err(UsageQueryError::InvalidCursor);
        }
        let streams = self.streams(&query, None).await?;
        let mut summary = UsageSummary::default();

        for stream in streams {
            let reader = self.store.read(&stream).await.map_err(storage_error)?;
            for (offset, line) in reader.lines().iter().enumerate() {
                let line_number = u64::try_from(offset + 1).unwrap_or(u64::MAX);
                if let Ok(record) = decode_record(
                    line.bytes(),
                    line.is_terminated(),
                    stream.as_str(),
                    line_number,
                ) {
                    if matches(&query, &record) {
                        add_summary(&mut summary, &record);
                    }
                }
            }
        }

        Ok(summary)
    }
}

impl UsageQueryService {
    async fn streams(
        &self,
        query: &UsageQuery,
        cursor: Option<&CursorPosition>,
    ) -> Result<Vec<AppendLogStream>, UsageQueryError> {
        let streams = if let Some(session_id) = &query.session_id {
            let target = AppendLogStream::for_session(session_id);
            self.store
                .list_streams(&AppendLogNamespace::usage())
                .await
                .map_err(storage_error)?
                .into_iter()
                .filter(|stream| stream == &target)
                .collect()
        } else {
            self.store
                .list_streams(&AppendLogNamespace::usage())
                .await
                .map_err(storage_error)?
        };
        let Some(cursor) = cursor else {
            return Ok(streams);
        };
        let position = streams
            .iter()
            .position(|stream| stream.as_str() == cursor.stream)
            .ok_or(UsageQueryError::InvalidCursor)?;
        Ok(streams.into_iter().skip(position).collect())
    }
}

fn storage_error(_: crate::AppendLogError) -> UsageQueryError {
    UsageQueryError::Storage("审计用量存储读取失败".to_string())
}
