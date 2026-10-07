use provider::{
    ModelInfo, ProviderContentData, ProviderError, ProviderErrorKind, ProviderRequestData,
    ProviderResponse, ProviderResponseChunk, ResponseStopReason, TokenUsageData,
};
use share::message::Message;
use share::reasoning::ReasoningLevel;

#[test]
fn crate_root_exposes_complete_provider_published_language_as_send_sync_values() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProviderRequestData>();
    assert_send_sync::<ModelInfo>();
    assert_send_sync::<ProviderContentData>();
    assert_send_sync::<ProviderError>();
    assert_send_sync::<ProviderErrorKind>();
    assert_send_sync::<ProviderResponse>();
    assert_send_sync::<ProviderResponseChunk>();
    assert_send_sync::<ResponseStopReason>();
    assert_send_sync::<TokenUsageData>();
}

#[test]
fn invocation_request_clone_shares_message_backing() {
    let request = ProviderRequestData::new(
        "contract-model".to_string(),
        vec![Message::user("history")],
        8_192,
        ReasoningLevel::Off,
    );
    let cloned = request.clone();

    assert_eq!(request.messages.as_ptr(), cloned.messages.as_ptr());
    assert_eq!(cloned.messages[0].text_content(), "history");
}

#[test]
fn crate_root_published_language_preserves_boundary_semantics() {
    let model = "contract-model".to_string();

    let request = ProviderRequestData::new(model, Vec::new(), 8_192, ReasoningLevel::Medium);
    assert!(request.system.is_empty());
    assert!(request.tools.is_empty());
    assert!(!request.cancellation.is_cancelled());

    let reported_zero = TokenUsageData {
        input_tokens: Some(0),
        ..TokenUsageData::default()
    };
    assert!(reported_zero.was_reported());
    assert_eq!(reported_zero.into_reported().unwrap().input_tokens, Some(0));
    assert!(TokenUsageData::default().into_reported().is_none());

    let cancelled = ProviderError::cancelled();
    assert_eq!(cancelled.kind, ProviderErrorKind::Cancelled);
    assert!(cancelled.is_cancelled());
    assert!(!cancelled.retryable);
    assert!(ProviderResponseChunk::Error(cancelled).is_terminal());
    assert!(
        !ProviderResponseChunk::Content(ProviderContentData::Text("x".to_string())).is_terminal()
    );
}
