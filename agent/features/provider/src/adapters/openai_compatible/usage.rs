use crate::domain::invoke::Usage;
use crate::RawUsageSnapshot;

fn optional_token_field(value: &serde_json::Value, field: &str) -> Option<u32> {
    value
        .get(field)
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
}

pub(crate) fn parse_chat_raw_usage(value: &serde_json::Value) -> RawUsageSnapshot {
    RawUsageSnapshot {
        input_tokens: optional_token_field(value, "prompt_tokens"),
        output_tokens: optional_token_field(value, "completion_tokens"),
        cache_read_tokens: nested_token_field(value, "prompt_tokens_details", "cached_tokens"),
        cache_write_tokens: None,
        reasoning_tokens: nested_token_field(
            value,
            "completion_tokens_details",
            "reasoning_tokens",
        ),
    }
}

pub(crate) fn parse_responses_raw_usage(value: &serde_json::Value) -> RawUsageSnapshot {
    RawUsageSnapshot {
        input_tokens: optional_token_field(value, "input_tokens"),
        output_tokens: optional_token_field(value, "output_tokens"),
        cache_read_tokens: nested_token_field(value, "input_tokens_details", "cached_tokens"),
        cache_write_tokens: None,
        reasoning_tokens: nested_token_field(value, "output_tokens_details", "reasoning_tokens"),
    }
}

fn token_field(value: &serde_json::Value, field: &str) -> u32 {
    value.get(field).and_then(|v| v.as_u64()).unwrap_or(0) as u32
}

fn nested_token_field(
    value: &serde_json::Value,
    details_field: &str,
    token_field: &str,
) -> Option<u32> {
    value
        .get(details_field)
        .and_then(|details| details.get(token_field))
        .and_then(|v| v.as_u64())
        .and_then(|value| u32::try_from(value).ok())
}

pub(super) fn parse_chat_usage(value: &serde_json::Value) -> Usage {
    let mut usage = Usage {
        input_tokens: token_field(value, "prompt_tokens"),
        output_tokens: token_field(value, "completion_tokens"),
        cached_tokens: nested_token_field(value, "prompt_tokens_details", "cached_tokens"),
        cache_creation_tokens: None,
        reasoning_tokens: nested_token_field(
            value,
            "completion_tokens_details",
            "reasoning_tokens",
        ),
        total_tokens: value
            .get("total_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
    };
    usage.finalize_total_tokens(0);
    usage
}

pub(super) fn parse_responses_usage(value: &serde_json::Value) -> Usage {
    let mut usage = Usage {
        input_tokens: token_field(value, "input_tokens"),
        output_tokens: token_field(value, "output_tokens"),
        cached_tokens: nested_token_field(value, "input_tokens_details", "cached_tokens"),
        cache_creation_tokens: None,
        reasoning_tokens: nested_token_field(value, "output_tokens_details", "reasoning_tokens"),
        total_tokens: value
            .get("total_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
    };
    usage.finalize_total_tokens(0);
    usage
}
