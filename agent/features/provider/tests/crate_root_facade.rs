use provider::{
    CapabilityFingerprint, InvocationDelta, InvocationEvent, InvocationOptions, InvocationRequest,
    ModelCapability, ModelId, ModelToolSchema, ProviderCompletion, ProviderContentBlock,
    ProviderError, ProviderErrorKind, ProviderStopReason, ProviderToolCall, ProviderToolCallId,
    RawUsageSnapshot, ReasoningCapability, ReasoningLevel, ReasoningMappingKind,
    RequestSystemBlock, RequestedInvocationOptions, ResolvedInvocationOptions,
};

#[test]
fn crate_root_exposes_complete_provider_published_language_as_send_sync_values() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CapabilityFingerprint>();
    assert_send_sync::<InvocationDelta>();
    assert_send_sync::<InvocationEvent>();
    assert_send_sync::<InvocationOptions>();
    assert_send_sync::<InvocationRequest>();
    assert_send_sync::<ModelCapability>();
    assert_send_sync::<ModelId>();
    assert_send_sync::<ModelToolSchema>();
    assert_send_sync::<ProviderCompletion>();
    assert_send_sync::<ProviderContentBlock>();
    assert_send_sync::<ProviderError>();
    assert_send_sync::<ProviderErrorKind>();
    assert_send_sync::<ProviderStopReason>();
    assert_send_sync::<ProviderToolCall>();
    assert_send_sync::<ProviderToolCallId>();
    assert_send_sync::<RawUsageSnapshot>();
    assert_send_sync::<ReasoningCapability>();
    assert_send_sync::<ReasoningLevel>();
    assert_send_sync::<ReasoningMappingKind>();
    assert_send_sync::<RequestSystemBlock>();
    assert_send_sync::<RequestedInvocationOptions>();
    assert_send_sync::<ResolvedInvocationOptions>();
}

#[test]
fn crate_root_published_language_preserves_boundary_semantics() {
    let model = ModelId {
        provider: "contract-provider".to_string(),
        model: "contract-model".to_string(),
    };
    let capability = ModelCapability {
        model: model.clone(),
        supports_tools: true,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        reasoning: ReasoningCapability::new(
            [ReasoningLevel::Off, ReasoningLevel::Medium],
            ReasoningMappingKind::Effort,
        )
        .expect("valid contract capability"),
        context_limit: Some(128_000),
        output_limit: Some(8_192),
    };
    let resolved = capability
        .resolve_invocation_options(RequestedInvocationOptions::new(16_384, ReasoningLevel::Max))
        .expect("public resolver contract");
    assert_eq!(resolved.context_size(), 128_000);
    assert_eq!(resolved.max_output_tokens(), 8_192);
    assert_eq!(resolved.requested_reasoning(), ReasoningLevel::Max);
    assert_eq!(resolved.effective_reasoning(), ReasoningLevel::Medium);
    assert_eq!(resolved.capability_fingerprint(), capability.fingerprint());

    let request = InvocationRequest::new(
        model,
        Vec::new(),
        InvocationOptions::new(8_192, ReasoningLevel::Medium),
    );
    assert!(request.system.is_empty());
    assert!(request.tools.is_empty());
    assert!(!request.cancellation.is_cancelled());

    let reported_zero = RawUsageSnapshot {
        input_tokens: Some(0),
        ..RawUsageSnapshot::default()
    };
    assert!(reported_zero.was_reported());
    assert_eq!(reported_zero.into_reported().unwrap().input_tokens, Some(0));
    assert!(RawUsageSnapshot::default().into_reported().is_none());

    let cancelled = ProviderError::cancelled();
    assert_eq!(cancelled.kind, ProviderErrorKind::Cancelled);
    assert!(cancelled.is_cancelled());
    assert!(!cancelled.retryable);
    assert!(InvocationEvent::Failed(cancelled).is_terminal());
    assert!(!InvocationEvent::Delta(InvocationDelta::Text("x".to_string())).is_terminal());
}
