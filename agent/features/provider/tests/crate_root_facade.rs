use provider::{
    InvocationDeltaData, InvocationEventData, InvocationOptionsData, InvocationRequestData,
    ModelCapabilityData, ModelIdData, ModelToolSchemaData, ProviderCompletionData,
    ProviderContentBlockData, ProviderError, ProviderErrorKind, ProviderStopReasonData,
    ProviderToolCallData, ProviderToolCallIdData, RawUsageSnapshotData, ReasoningCapabilityData,
    ReasoningMappingKindData, RequestSystemBlockData,
};
use share::message::Message;
use share::reasoning::ReasoningLevel;

#[test]
fn crate_root_exposes_complete_provider_published_language_as_send_sync_values() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<InvocationDeltaData>();
    assert_send_sync::<InvocationEventData>();
    assert_send_sync::<InvocationOptionsData>();
    assert_send_sync::<InvocationRequestData>();
    assert_send_sync::<ModelCapabilityData>();
    assert_send_sync::<ModelIdData>();
    assert_send_sync::<ModelToolSchemaData>();
    assert_send_sync::<ProviderCompletionData>();
    assert_send_sync::<ProviderContentBlockData>();
    assert_send_sync::<ProviderError>();
    assert_send_sync::<ProviderErrorKind>();
    assert_send_sync::<ProviderStopReasonData>();
    assert_send_sync::<ProviderToolCallData>();
    assert_send_sync::<ProviderToolCallIdData>();
    assert_send_sync::<RawUsageSnapshotData>();
    assert_send_sync::<ReasoningCapabilityData>();
    assert_send_sync::<ReasoningMappingKindData>();
    assert_send_sync::<RequestSystemBlockData>();
}

#[test]
fn invocation_request_clone_shares_message_backing() {
    let request = InvocationRequestData::new(
        ModelIdData {
            provider: "contract-provider".to_string(),
            model: "contract-model".to_string(),
        },
        vec![Message::user("history")],
        InvocationOptionsData::new(8_192, ReasoningLevel::Off),
    );
    let cloned = request.clone();

    assert_eq!(request.messages.as_ptr(), cloned.messages.as_ptr());
    assert_eq!(cloned.messages[0].text_content(), "history");
}

#[test]
fn crate_root_published_language_preserves_boundary_semantics() {
    let model = ModelIdData {
        provider: "contract-provider".to_string(),
        model: "contract-model".to_string(),
    };

    let request = InvocationRequestData::new(
        model,
        Vec::new(),
        InvocationOptionsData::new(8_192, ReasoningLevel::Medium),
    );
    assert!(request.system.is_empty());
    assert!(request.tools.is_empty());
    assert!(!request.cancellation.is_cancelled());

    let reported_zero = RawUsageSnapshotData {
        input_tokens: Some(0),
        ..RawUsageSnapshotData::default()
    };
    assert!(reported_zero.was_reported());
    assert_eq!(reported_zero.into_reported().unwrap().input_tokens, Some(0));
    assert!(RawUsageSnapshotData::default().into_reported().is_none());

    let cancelled = ProviderError::cancelled();
    assert_eq!(cancelled.kind, ProviderErrorKind::Cancelled);
    assert!(cancelled.is_cancelled());
    assert!(!cancelled.retryable);
    assert!(InvocationEventData::Failed(cancelled).is_terminal());
    assert!(!InvocationEventData::Delta(InvocationDeltaData::Text("x".to_string())).is_terminal());
}
