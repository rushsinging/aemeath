use super::published_language::{ProviderError, ProviderErrorKind};
use super::LlmError;

#[test]
fn llm_cancelled_error_is_classified_as_cancelled() {
    assert!(LlmError::Cancelled.is_cancelled());
}

#[test]
fn llm_stream_truncated_error_is_recognized_structurally() {
    let error = LlmError::StreamTruncated {
        tool_call_id: "call_x".to_string(),
        tool_call_name: "Write".to_string(),
        accumulated_bytes: 31428,
        delta_count: 3468,
        head_preview: "{\"file_path\":\"/x\"".to_string(),
        tail_preview: "...truncated...".to_string(),
    };
    assert!(error.is_stream_truncated());
    let rendered = format!("{error}");
    assert!(rendered.contains("Write"));
    assert!(rendered.contains("call_x"));
    assert!(rendered.contains("31428"));
}

#[test]
fn llm_non_stream_truncated_errors_are_not_misclassified() {
    assert!(!LlmError::Cancelled.is_stream_truncated());
    assert!(!LlmError::Stream("some other failure".to_string()).is_stream_truncated());
    assert!(!LlmError::Network("reset".to_string()).is_stream_truncated());
    assert!(!LlmError::RateLimited.is_stream_truncated());
}

/// `From<LlmError> for ProviderError` 是 crate 内三 driver + stream +
/// runtime 装配共用的权威映射（composition 侧手抄副本已收敛至本实现），
/// 逐变体锁定 kind 归类与 fatal 语义，防止变体增删时静默漂移。
#[test]
fn llm_error_to_provider_error_maps_every_variant_kind() {
    let stream_truncated = LlmError::StreamTruncated {
        tool_call_id: "call_x".to_string(),
        tool_call_name: "Write".to_string(),
        accumulated_bytes: 128,
        delta_count: 3,
        head_preview: "{\"file_path\"".to_string(),
        tail_preview: "...".to_string(),
    };
    let cases: Vec<(LlmError, ProviderErrorKind)> = vec![
        (LlmError::Cancelled, ProviderErrorKind::Cancelled),
        (LlmError::RateLimited, ProviderErrorKind::RateLimited),
        (LlmError::ContextTooLong, ProviderErrorKind::ContextTooLong),
        (
            LlmError::Network("reset".to_string()),
            ProviderErrorKind::Network,
        ),
        (
            LlmError::Api {
                error_type: "invalid_request_error".to_string(),
                message: "bad request".to_string(),
            },
            ProviderErrorKind::UpstreamUnavailable,
        ),
        (
            LlmError::Stream("decode failure".to_string()),
            ProviderErrorKind::Protocol,
        ),
        (
            LlmError::StreamInterrupted("eof mid-stream".to_string()),
            ProviderErrorKind::StreamTruncated,
        ),
        (stream_truncated, ProviderErrorKind::StreamTruncated),
        (
            LlmError::Config("missing api key".to_string()),
            ProviderErrorKind::Configuration,
        ),
    ];
    for (error, expected_kind) in cases {
        let mapped = ProviderError::from(error);
        assert_eq!(mapped.kind, expected_kind);
        assert!(!mapped.retryable, "From 映射固定为 fatal 语义");
        assert!(!mapped.safe_message.is_empty());
    }
}
