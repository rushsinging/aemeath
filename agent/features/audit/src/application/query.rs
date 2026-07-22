use crate::{
    TimeRange, UsageEnvelopeV1, UsageQuery, UsageQueryError, UsageQueryWarning, UsageRecord,
    UsageSummary, CURRENT_USAGE_SCHEMA_VERSION,
};

pub(crate) const MAX_USAGE_QUERY_LIMIT: usize = 1_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorPosition {
    pub stream: String,
    pub next_line_offset: usize,
    pub query_fingerprint: String,
}

pub(crate) fn validate_query(query: &UsageQuery) -> Result<usize, UsageQueryError> {
    if let Some(TimeRange {
        from_inclusive_unix_ms: Some(from),
        to_exclusive_unix_ms: Some(to),
    }) = query.recorded_range
    {
        if from >= to {
            return Err(UsageQueryError::InvalidRange);
        }
    }
    Ok(query.pagination.limit.get().min(MAX_USAGE_QUERY_LIMIT))
}

pub(crate) fn decode_cursor(value: &str) -> Result<CursorPosition, UsageQueryError> {
    let Some((encoded_fingerprint, position)) = value
        .strip_prefix("v1:")
        .and_then(|value| value.split_once(':'))
    else {
        return Err(UsageQueryError::InvalidCursor);
    };
    let Some((encoded_stream, offset)) = position.rsplit_once(':') else {
        return Err(UsageQueryError::InvalidCursor);
    };
    let query_fingerprint =
        hex_decode(encoded_fingerprint).ok_or(UsageQueryError::InvalidCursor)?;
    let stream = hex_decode(encoded_stream).ok_or(UsageQueryError::InvalidCursor)?;
    let next_line_offset = offset.parse().map_err(|_| UsageQueryError::InvalidCursor)?;
    if stream.is_empty() {
        return Err(UsageQueryError::InvalidCursor);
    }
    Ok(CursorPosition {
        stream,
        next_line_offset,
        query_fingerprint,
    })
}

pub(crate) fn query_fingerprint(query: &UsageQuery) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        query.session_id,
        query.run_id,
        query.run_step_id,
        query.model_invocation_id,
        query.provider,
        query.model,
        query.recorded_range,
    )
}

pub(crate) fn encode_cursor(position: &CursorPosition) -> String {
    format!(
        "v1:{}:{}:{}",
        hex_encode(&position.query_fingerprint),
        hex_encode(&position.stream),
        position.next_line_offset
    )
}

pub(crate) fn decode_record(
    bytes: &[u8],
    terminated: bool,
    stream: &str,
    line_number: u64,
) -> Result<UsageRecord, UsageQueryWarning> {
    if !terminated {
        return Err(corrupt(stream, line_number));
    }
    let envelope: UsageEnvelopeV1 =
        serde_json::from_slice(bytes).map_err(|_| corrupt(stream, line_number))?;
    if envelope.schema_version != CURRENT_USAGE_SCHEMA_VERSION {
        return Err(corrupt(stream, line_number));
    }
    Ok(envelope.record)
}

pub(crate) fn matches(query: &UsageQuery, record: &UsageRecord) -> bool {
    query
        .session_id
        .as_ref()
        .is_none_or(|value| value == &record.session_id)
        && query
            .run_id
            .as_ref()
            .is_none_or(|value| value == &record.run_id)
        && query
            .run_step_id
            .as_ref()
            .is_none_or(|value| value == &record.run_step_id)
        && query
            .model_invocation_id
            .as_ref()
            .is_none_or(|value| value == &record.model_invocation_id)
        && query
            .provider
            .as_ref()
            .is_none_or(|value| value == &record.provider)
        && query
            .model
            .as_ref()
            .is_none_or(|value| value == &record.model)
        && query.recorded_range.is_none_or(|range| {
            range
                .from_inclusive_unix_ms
                .is_none_or(|from| record.recorded_at_unix_ms >= from)
                && range
                    .to_exclusive_unix_ms
                    .is_none_or(|to| record.recorded_at_unix_ms < to)
        })
}

pub(crate) fn add_summary(summary: &mut UsageSummary, record: &UsageRecord) {
    summary.record_count += 1;
    summary.input_tokens += record.input_tokens;
    summary.output_tokens += record.output_tokens;
    summary.cache_write_tokens += record.cache_write_tokens.unwrap_or(0);
    summary.cache_read_tokens += record.cache_read_tokens.unwrap_or(0);
    summary.reasoning_tokens += record.reasoning_tokens.unwrap_or(0);
}

fn corrupt(stream: &str, line_number: u64) -> UsageQueryWarning {
    UsageQueryWarning::CorruptLine {
        stream: stream.to_string(),
        line_number,
    }
}

fn hex_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hex_decode(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}
