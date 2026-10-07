use super::*;

#[test]
fn model_id_display() {
    let id = ModelIdData {
        provider: "Anthropic".to_string(),
        model: "claude-sonnet-4".to_string(),
    };
    assert_eq!(id.to_string(), "Anthropic/claude-sonnet-4");
}

#[test]
fn provider_error_cancelled() {
    let e = ProviderError::cancelled();
    assert!(e.is_cancelled());
    assert!(!e.retryable);
    assert_eq!(e.kind, ProviderErrorKind::Cancelled);
}

#[test]
fn provider_error_context_exceeded() {
    let e = ProviderError::fatal(ProviderErrorKind::ContextTooLong, "context length exceeded");
    assert!(e.is_context_exceeded());
    assert!(!e.retryable);
}

#[test]
fn provider_error_retryable() {
    let e = ProviderError::retryable(ProviderErrorKind::RateLimited, "429");
    assert!(e.retryable);
    assert_eq!(e.kind, ProviderErrorKind::RateLimited);
}

#[test]
fn reasoning_capability_none() {
    let cap = ReasoningCapabilityData::none();
    assert_eq!(cap.supported(), &[ReasoningLevel::Off]);
    assert_eq!(cap.maximum(), ReasoningLevel::Off);
    assert_eq!(cap.mapping, ReasoningMappingKindData::None);
}

#[test]
fn resolver_selects_highest_supported_level_not_above_requested() {
    let capability = ModelCapabilityData {
        model: ModelIdData {
            provider: "fake".to_string(),
            model: "sparse-levels".to_string(),
        },
        supports_tools: true,
        supports_parallel_tool_calls: true,
        supports_streaming: true,
        reasoning: ReasoningCapabilityData::new(
            [
                ReasoningLevel::Off,
                ReasoningLevel::Medium,
                ReasoningLevel::Max,
            ],
            ReasoningMappingKindData::Effort,
        )
        .expect("valid sparse capability"),
        context_limit: Some(128_000),
        output_limit: Some(8_192),
    };

    for (requested, expected) in [
        (ReasoningLevel::Off, ReasoningLevel::Off),
        (ReasoningLevel::Minimal, ReasoningLevel::Off),
        (ReasoningLevel::Low, ReasoningLevel::Off),
        (ReasoningLevel::Medium, ReasoningLevel::Medium),
        (ReasoningLevel::High, ReasoningLevel::Medium),
        (ReasoningLevel::Xhigh, ReasoningLevel::Medium),
        (ReasoningLevel::Max, ReasoningLevel::Max),
    ] {
        let effective = capability.reasoning.resolve(requested);
        assert_eq!(effective, expected);
        assert!(effective <= requested);
    }
}

#[test]
fn resolver_preserves_minimal_and_max_when_capability_declares_minimal() {
    // OpenAI driver 的 capability 显式声明七档；resolver 必须把 Minimal 与
    // Max 原样下传，证明共享枚举新增档位不会自动丢失。
    let openai = ReasoningCapabilityData::new(
        [
            ReasoningLevel::Off,
            ReasoningLevel::Minimal,
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
            ReasoningLevel::High,
            ReasoningLevel::Xhigh,
            ReasoningLevel::Max,
        ],
        ReasoningMappingKindData::Effort,
    )
    .expect("OpenAI capability includes off and seven levels");

    assert_eq!(
        openai.resolve(ReasoningLevel::Minimal),
        ReasoningLevel::Minimal
    );
    assert_eq!(openai.resolve(ReasoningLevel::Max), ReasoningLevel::Max);
}

#[test]
fn resolver_downgrades_minimal_to_off_when_capability_omits_minimal() {
    // Legacy driver（如 Zhipu/LiteLLM）的 capability 不包含 Minimal：
    // resolver 必须把 Minimal 向下退到 Off，禁止把 Off 静默升级到 Minimal，
    // 也禁止因为 Minimal 不在集合里而 panic 或返回任何非 Off 档位。
    let legacy = ReasoningCapabilityData::new(
        [
            ReasoningLevel::Off,
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
        ],
        ReasoningMappingKindData::Effort,
    )
    .expect("legacy capability includes off");

    assert_eq!(legacy.resolve(ReasoningLevel::Minimal), ReasoningLevel::Off);
}

#[test]
fn reasoning_capability_rejects_empty_or_missing_off_levels() {
    assert!(ReasoningCapabilityData::new([], ReasoningMappingKindData::None).is_err());
    assert!(ReasoningCapabilityData::new(
        [ReasoningLevel::Medium],
        ReasoningMappingKindData::Effort,
    )
    .is_err());
}

#[test]
fn stop_reason_variants() {
    let r = StopReason::EndTurn;
    assert_eq!(r, StopReason::EndTurn);

    let other = StopReason::Other("unknown".to_string());
    assert!(matches!(other, StopReason::Other(_)));
}

#[test]
fn provider_tool_call_id_display() {
    let id = ProviderToolCallIdData("toolu_123".to_string());
    assert_eq!(id.to_string(), "toolu_123");
}

#[test]
fn raw_usage_distinguishes_unreported_from_reported_zero() {
    let unreported = RawUsageSnapshotData::default();
    assert!(!unreported.was_reported());
    assert!(unreported.into_reported().is_none());

    let reported_zero = RawUsageSnapshotData {
        input_tokens: Some(0),
        ..RawUsageSnapshotData::default()
    };
    assert!(reported_zero.was_reported());
    assert_eq!(reported_zero.into_reported().unwrap().input_tokens, Some(0));
}

#[test]
fn raw_usage_latest_reported_fields_merge_without_erasing_previous_values() {
    let mut usage = RawUsageSnapshotData {
        input_tokens: Some(10),
        cache_read_tokens: Some(3),
        ..RawUsageSnapshotData::default()
    };
    usage.merge_reported(RawUsageSnapshotData {
        output_tokens: Some(7),
        cache_read_tokens: None,
        reasoning_tokens: Some(0),
        ..RawUsageSnapshotData::default()
    });

    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(7));
    assert_eq!(usage.cache_read_tokens, Some(3));
    assert_eq!(usage.reasoning_tokens, Some(0));
}

#[test]
fn raw_usage_snapshot_default_all_none() {
    let usage = RawUsageSnapshotData::default();
    assert!(usage.input_tokens.is_none());
    assert!(usage.output_tokens.is_none());
    assert!(usage.cache_read_tokens.is_none());
}

#[test]
fn invocation_request_new_has_empty_tools() {
    let req = InvocationRequestData::new(
        ModelIdData {
            provider: "test".to_string(),
            model: "m".to_string(),
        },
        Vec::new(),
        8192,
        ReasoningLevel::Off,
    );
    assert!(req.tools.is_empty());
}

#[test]
fn invocation_request_new_has_empty_system() {
    let req = InvocationRequestData::new(
        ModelIdData {
            provider: "test".to_string(),
            model: "m".to_string(),
        },
        Vec::new(),
        8192,
        ReasoningLevel::Off,
    );
    assert!(req.system.is_empty());
}

#[test]
fn request_system_block_exposes_text_and_cacheable_flag() {
    let dynamic = RequestSystemBlockData::Text("dynamic".to_string());
    assert_eq!(dynamic.text(), "dynamic");
    assert!(!dynamic.is_cacheable());

    let cached = RequestSystemBlockData::Cacheable("static".to_string());
    assert_eq!(cached.text(), "static");
    assert!(cached.is_cacheable());
}

#[test]
fn invocation_event_delta_is_non_terminal() {
    let evt = InvocationEventData::Delta(InvocationDeltaData::Text("hi".to_string()));
    assert!(!evt.is_terminal());
}

#[test]
fn invocation_event_completed_and_failed_are_terminal() {
    let completion = ProviderCompletionData {
        output: Vec::new(),
        stop_reason: StopReason::EndTurn,
        usage: None,
        effective_reasoning: ReasoningLevel::Off,
    };
    assert!(InvocationEventData::Completed(completion).is_terminal());
    assert!(InvocationEventData::Failed(ProviderError::cancelled()).is_terminal());
}

#[test]
fn tool_call_identity_can_bind_provider_id_after_start() {
    let started = InvocationDeltaData::ToolCallStarted {
        index: 2,
        provider_id: None,
        name: "Write".to_string(),
    };
    let arguments = InvocationDeltaData::ToolArgumentsDelta {
        index: 2,
        provider_id: Some(ProviderToolCallIdData("call_late".to_string())),
        partial_json: "{}".to_string(),
    };

    assert!(matches!(
        started,
        InvocationDeltaData::ToolCallStarted {
            index: 2,
            provider_id: None,
            ..
        }
    ));
    assert!(matches!(
        arguments,
        InvocationDeltaData::ToolArgumentsDelta {
            index: 2,
            provider_id: Some(ProviderToolCallIdData(ref id)),
            ..
        } if id == "call_late"
    ));
}
