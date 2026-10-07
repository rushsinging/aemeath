use super::*;

#[test]
fn model_info_unifies_identity_and_capability() {
    // #1880：model 信息只有一个实体来源——身份与能力不可分。
    let info = ModelInfo {
        provider: "Anthropic".to_string(),
        model: "claude-sonnet-4".to_string(),
        supports_tools: true,
        supports_parallel_tool_calls: true,
        supports_streaming: true,
        supported_reasoning: vec![ReasoningLevel::Off],
        context_limit: Some(200_000),
        output_limit: Some(8_192),
    };
    assert_eq!(info.provider, "Anthropic");
    assert_eq!(info.model, "claude-sonnet-4");
    assert!(info.supports_tools);
    assert_eq!(info.context_limit, Some(200_000));
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
    let info = ModelInfo {
        provider: "fake".to_string(),
        model: "off-only".to_string(),
        supports_tools: false,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        supported_reasoning: vec![ReasoningLevel::Off],
        context_limit: None,
        output_limit: None,
    };
    assert_eq!(info.supported_reasoning, &[ReasoningLevel::Off]);
    assert_eq!(
        info.resolve_reasoning(ReasoningLevel::High),
        ReasoningLevel::Off
    );
}

#[test]
fn resolver_selects_highest_supported_level_not_above_requested() {
    let capability = ModelInfo {
        provider: "fake".to_string(),
        model: "sparse-levels".to_string(),
        supports_tools: true,
        supports_parallel_tool_calls: true,
        supports_streaming: true,
        supported_reasoning: vec![
            ReasoningLevel::Off,
            ReasoningLevel::Medium,
            ReasoningLevel::Max,
        ],
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
        let effective = capability.resolve_reasoning(requested);
        assert_eq!(effective, expected);
        assert!(effective <= requested);
    }
}

/// 测试用 ModelInfo：身份固定，仅阶梯按入参构造（#1861 v4 摊平后
/// resolve 行为直接锁在实体方法上）。
fn model_with_supported(supported_reasoning: Vec<ReasoningLevel>) -> ModelInfo {
    ModelInfo {
        provider: "fake".to_string(),
        model: "capability".to_string(),
        supports_tools: true,
        supports_parallel_tool_calls: true,
        supports_streaming: true,
        supported_reasoning,
        context_limit: Some(200_000),
        output_limit: Some(8_192),
    }
}

#[test]
fn resolver_preserves_minimal_and_max_when_capability_declares_minimal() {
    // OpenAI driver 的 capability 显式声明七档；resolver 必须把 Minimal 与
    // Max 原样下传，证明共享枚举新增档位不会自动丢失。
    let openai = model_with_supported(vec![
        ReasoningLevel::Off,
        ReasoningLevel::Minimal,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
        ReasoningLevel::Xhigh,
        ReasoningLevel::Max,
    ]);

    assert_eq!(
        openai.resolve_reasoning(ReasoningLevel::Minimal),
        ReasoningLevel::Minimal
    );
    assert_eq!(
        openai.resolve_reasoning(ReasoningLevel::Max),
        ReasoningLevel::Max
    );
}

#[test]
fn resolver_downgrades_minimal_to_off_when_capability_omits_minimal() {
    // Legacy driver（如 Zhipu/LiteLLM）的 capability 不包含 Minimal：
    // resolver 必须把 Minimal 向下退到 Off，禁止把 Off 静默升级到 Minimal，
    // 也禁止因为 Minimal 不在集合里而 panic 或返回任何非 Off 档位。
    let legacy = model_with_supported(vec![
        ReasoningLevel::Off,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
    ]);

    assert_eq!(
        legacy.resolve_reasoning(ReasoningLevel::Minimal),
        ReasoningLevel::Off
    );
}

#[test]
fn resolver_falls_back_to_off_for_empty_or_missing_off_ladders() {
    // 阶梯缺失 Off 甚至为空时，resolve 永远以 Off 兜底（不 panic）。
    let empty = model_with_supported(vec![]);
    assert_eq!(
        empty.resolve_reasoning(ReasoningLevel::Medium),
        ReasoningLevel::Off
    );

    let missing_off = model_with_supported(vec![ReasoningLevel::Medium]);
    assert_eq!(
        missing_off.resolve_reasoning(ReasoningLevel::Low),
        ReasoningLevel::Off
    );
    assert_eq!(
        missing_off.resolve_reasoning(ReasoningLevel::Max),
        ReasoningLevel::Medium
    );
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
    let id = "toolu_123".to_string();
    assert_eq!(id.to_string(), "toolu_123");
}

#[test]
fn raw_usage_distinguishes_unreported_from_reported_zero() {
    let unreported = TokenUsageData::default();
    assert!(!unreported.was_reported());
    assert!(unreported.into_reported().is_none());

    let reported_zero = TokenUsageData {
        input_tokens: Some(0),
        ..TokenUsageData::default()
    };
    assert!(reported_zero.was_reported());
    assert_eq!(reported_zero.into_reported().unwrap().input_tokens, Some(0));
}

#[test]
fn raw_usage_latest_reported_fields_merge_without_erasing_previous_values() {
    let mut usage = TokenUsageData {
        input_tokens: Some(10),
        cache_read_tokens: Some(3),
        ..TokenUsageData::default()
    };
    usage.merge_reported(TokenUsageData {
        output_tokens: Some(7),
        cache_read_tokens: None,
        reasoning_tokens: Some(0),
        ..TokenUsageData::default()
    });

    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(7));
    assert_eq!(usage.cache_read_tokens, Some(3));
    assert_eq!(usage.reasoning_tokens, Some(0));
}

#[test]
fn raw_usage_snapshot_default_all_none() {
    let usage = TokenUsageData::default();
    assert!(usage.input_tokens.is_none());
    assert!(usage.output_tokens.is_none());
    assert!(usage.cache_read_tokens.is_none());
}

#[test]
fn invocation_request_new_has_empty_tools() {
    let req = ProviderRequestData::new("m".to_string(), Vec::new(), 8192, ReasoningLevel::Off);
    assert!(req.tools.is_empty());
}

#[test]
fn invocation_request_new_has_empty_system() {
    let req = ProviderRequestData::new("m".to_string(), Vec::new(), 8192, ReasoningLevel::Off);
    assert!(req.system.is_empty());
}

#[test]
fn invocation_event_content_is_non_terminal() {
    let evt = ProviderResponseChunk::Content(ProviderContentData::Text("hi".to_string()));
    assert!(!evt.is_terminal());
}

#[test]
fn invocation_event_stop_and_error_are_terminal() {
    assert!(ProviderResponseChunk::Stop(StopReason::EndTurn).is_terminal());
    assert!(ProviderResponseChunk::Error(ProviderError::cancelled()).is_terminal());
    // Usage 只是归位片段，流仍在继续。
    assert!(!ProviderResponseChunk::Usage(TokenUsageData::default()).is_terminal());
}

#[test]
fn tool_call_identity_can_bind_provider_id_after_start() {
    let started = ProviderContentData::ToolCallStarted {
        index: 2,
        provider_id: None,
        name: "Write".to_string(),
    };
    let arguments = ProviderContentData::ToolArgumentsDelta {
        index: 2,
        provider_id: Some("call_late".to_string()),
        partial_json: "{}".to_string(),
    };

    assert!(matches!(
        started,
        ProviderContentData::ToolCallStarted {
            index: 2,
            provider_id: None,
            ..
        }
    ));
    assert!(matches!(
        arguments,
        ProviderContentData::ToolArgumentsDelta {
            index: 2,
            provider_id: Some(ref id),
            ..
        } if id == "call_late"
    ));
}
