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
