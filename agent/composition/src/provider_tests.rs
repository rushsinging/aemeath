// ─── 迁移记录（provider invoke 转换断言组 → provider crate）────────────
//
// v2 后 ProviderAdapter::invoke 是纯直传：system block / tool schema 转换、
// reasoning clamp、ResolvedInvocation 校验（max_tokens=0 → Configuration）、
// 底层错误透传与 establishment 取消竞速全部归 provider crate 的
// LlmClient::invoke。原 FakeLlmProvider（impl 已 pub(crate) 的 LlmProvider）
// 基建随构造面撤空一并删除。以下测试已迁移至
// agent/features/provider/src/adapters/client_invoke_tests.rs（断言语义等价）：
//   - invoke_returns_stream_with_delta_then_completed（Delta→Completed 流序）
//   - invoke_propagates_provider_error（底层错误透传）
//   - invoke_rejects_invalid_scope（ResolvedInvocation 校验 → Configuration）
//   - invoke_converts_system_blocks_tools_and_uses_neutral_scope_model（system/tool 转换）
//   - invoke_clamps_requested_reasoning_to_capability（capability clamp）
//   - invoke_invokes_provider_exactly_once（单次 invoke 单次上游调用）
//   - invoke_returns_cancelled_when_signal_fires_during_establishment（establishment 取消竞速）
// 本文件保留 adapter 自有逻辑测试：capability 查表（capabilities_*、
// invoke_rejects_unknown_model）、invoke fast-path（信号已置位直返 Cancelled）、
// factory 装配与 transport pool 复用契约。

use super::*;
use provider::composition::{wire_provider_assembly, LlmConfigOptionsData, TransportPool};
use provider::{
    InvocationRequestData, ModelCapabilityData, ModelIdData, ProviderErrorKind,
    ReasoningCapabilityData,
};
use share::reasoning::ReasoningLevel;
use tokio_util::sync::CancellationToken;

// ─── Helpers ──────────────────────────────────────────────────────────

fn test_model_id() -> ModelIdData {
    ModelIdData {
        provider: "fake-provider".to_string(),
        model: "fake-model".to_string(),
    }
}

fn test_capability() -> ModelCapabilityData {
    ModelCapabilityData {
        model: test_model_id(),
        supports_tools: true,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        reasoning: ReasoningCapabilityData::none(),
        context_limit: Some(128_000),
        output_limit: Some(8_192),
    }
}

/// Build a port over a real (wire-assembled) client and the given capability.
/// Adapter 自有逻辑测试只走 capability 查表与 fast-path，不触发上游调用；
/// LlmProvider 构造面已撤空（见文件头迁移记录），客户端经组合根唯一装配
/// 入口 `wire_provider_assembly` 构造。
fn build_port_with_capability(
    capability: ModelCapabilityData,
) -> (Arc<dyn ProviderPort>, ModelIdData) {
    let model = capability.model.clone();
    let client = wire_provider_assembly(
        LlmConfigOptionsData {
            driver: "openai".to_string(),
            source_key: "test-source".to_string(),
            api_style: None,
            api_key: "test-api-key".to_string(),
            base_url: Some("https://example.test/v1".to_string()),
            model: model.model.clone(),
            max_tokens: 8192,
            reasoning: false,
            reasoning_config: None,
            timeout_secs: 30,
            user_agent: Some("aemeath-test/1.0".to_string()),
        },
        model.clone(),
        &TransportPool::new(),
        ReasoningLevel::Off,
        Some(128_000),
        8_192,
    )
    .expect("test client must wire through the composition entry")
    .client;
    let caps = HashMap::from([(model.clone(), capability)]);
    (provider_port(client, caps), model)
}

/// Build a port for tests that only exercise adapter-owned behaviour.
fn build_port() -> (Arc<dyn ProviderPort>, ModelIdData) {
    build_port_with_capability(test_capability())
}

// ─── Tests ────────────────────────────────────────────────────────────

#[test]
fn factory_returns_provider_port_that_is_send_sync() {
    fn assert_send_sync<T: Send + Sync + ?Sized>(_: &T) {}

    let (port, _) = build_port();
    assert_send_sync(port.as_ref());
}

#[test]
fn capabilities_returns_for_known_model() {
    let (port, model) = build_port();

    let cap = port.capabilities(&model).unwrap();
    assert!(cap.supports_tools);
    assert!(!cap.supports_parallel_tool_calls);
    assert!(cap.supports_streaming);
    assert_eq!(cap.context_limit, Some(128_000));
    assert_eq!(cap.output_limit, Some(8_192));
}

#[test]
fn capabilities_rejects_unknown_model() {
    let (port, _) = build_port();

    let unknown = ModelIdData {
        provider: "unknown".to_string(),
        model: "x".to_string(),
    };
    let err = port.capabilities(&unknown).unwrap_err();
    assert_eq!(err.kind, ProviderErrorKind::ModelUnavailable);
    assert!(!err.retryable);
}

#[tokio::test]
async fn invoke_returns_cancelled_when_signal_already_set() {
    let (port, model) = build_port();

    let request = InvocationRequestData::new(model, vec![], 8192, ReasoningLevel::Off);
    let cancel = CancellationToken::new();
    cancel.cancel();

    let result = port.invoke(request, &cancel).await;
    assert!(
        matches!(result, Err(ref e) if e.is_cancelled()),
        "expected cancelled error"
    );
}

#[tokio::test]
async fn invoke_rejects_unknown_model() {
    let (port, _known) = build_port();

    let unknown = ModelIdData {
        provider: "nope".to_string(),
        model: "ghost".to_string(),
    };
    let request = InvocationRequestData::new(unknown, vec![], 8192, ReasoningLevel::Off);
    let cancel = CancellationToken::new();

    let result = port.invoke(request, &cancel).await;
    assert!(
        matches!(result, Err(ref e) if e.kind == ProviderErrorKind::ModelUnavailable),
        "expected ModelUnavailable for a model with no declared capability"
    );
}

// ─── ProviderFactory TDD tests ─────────────────────────────────────

fn valid_spec() -> ProviderBuildSpecData {
    ProviderBuildSpecData {
        driver: "anthropic".to_string(),
        source_key: "test-source".to_string(),
        api_style: None,
        api_key: "sk-test-key".to_string(),
        base_url: Some("https://api.anthropic.com".to_string()),
        model: ModelIdData {
            provider: "Anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
        },
        max_tokens: 8192,
        requested_reasoning: ReasoningLevel::Off,
        context_window: Some(200_000),
        timeout: std::time::Duration::from_secs(60),
        user_agent: "aemeath-test/1.0".to_string(),
    }
}

#[test]
fn factory_build_valid_spec_returns_binding() {
    let factory = super::provider_factory();
    let spec = valid_spec();

    let binding = factory
        .build(spec.clone())
        .expect("valid spec should build");

    assert_eq!(binding.model, spec.model);
    assert_eq!(binding.max_tokens, spec.max_tokens);
    assert_eq!(binding.context_window, spec.context_window);
    assert_eq!(
        binding.requested_reasoning,
        ReasoningLevel::Off,
        "reasoning=false => Off"
    );

    // The binding's provider port must be callable.
    let cap = binding
        .provider
        .capabilities(&binding.model)
        .expect("capabilities for the built model");
    assert!(cap.supports_tools);
    assert!(cap.supports_streaming);
    assert_eq!(cap.context_limit, spec.context_window);
    assert_eq!(cap.output_limit, Some(spec.max_tokens as usize));
}

#[test]
fn factory_build_missing_endpoint_fails_closed() {
    let factory = super::provider_factory();
    let mut spec = valid_spec();
    spec.base_url = None;

    let err = factory
        .build(spec)
        .expect_err("Composition 必须拒绝未由 Config 解析的 endpoint");
    assert_eq!(err.kind, ProviderErrorKind::Configuration);
    assert!(err.safe_message.contains("base URL"));
}

#[test]
fn factory_build_invalid_driver_fails_closed() {
    let factory = super::provider_factory();
    let mut spec = valid_spec();
    spec.driver = "nonexistent-driver-xyz".to_string();

    let err = factory
        .build(spec)
        .expect_err("unknown driver must fail closed");

    assert_eq!(
        err.kind,
        ProviderErrorKind::Configuration,
        "unknown driver => Configuration error"
    );
    assert!(!err.retryable);
    assert!(
        err.safe_message.contains("nonexistent-driver-xyz")
            || err.safe_message.contains("UnknownDriver")
            || err.safe_message.contains("unknown"),
        "error message should mention the driver: {}",
        err.safe_message
    );
}

#[test]
fn factory_build_empty_driver_fails_closed() {
    let factory = super::provider_factory();
    let mut spec = valid_spec();
    spec.driver = String::new();

    let err = factory
        .build(spec)
        .expect_err("empty driver must fail closed");
    assert_eq!(err.kind, ProviderErrorKind::Configuration);
    assert!(!err.retryable);
}

#[test]
fn factory_build_preserves_requested_reasoning() {
    let factory = super::provider_factory();
    let mut spec = valid_spec();
    spec.requested_reasoning = ReasoningLevel::High;
    // Use "openai" which supports reasoning via Effort mapping.
    spec.driver = "openai".to_string();
    spec.model = ModelIdData {
        provider: "OpenAI".to_string(),
        model: "gpt-4o".to_string(),
    };

    let binding = factory.build(spec).expect("valid spec with reasoning");

    // reasoning=true without reasoning_config maps to High by default.
    assert_eq!(
        binding.requested_reasoning,
        ReasoningLevel::High,
        "reasoning=true => High"
    );
}

#[test]
fn factory_build_produces_send_sync_binding() {
    fn assert_send_sync<T: Send + Sync + ?Sized>(_: &T) {}

    let factory = super::provider_factory();
    let spec = valid_spec();
    let binding = factory.build(spec).expect("valid spec");

    assert_send_sync(&binding);
    assert_send_sync(binding.provider.as_ref());
}

#[test]
fn provider_build_spec_is_clone_and_debug() {
    let spec = valid_spec();
    let _cloned = spec.clone();
    let _ = format!("{spec:?}");
}

// ─── Transport pool reuse contract ────────────────────────────────────

fn spec_with(
    model: &str,
    driver: &str,
    api_key: &str,
    base_url: Option<&str>,
    user_agent: &str,
) -> ProviderBuildSpecData {
    ProviderBuildSpecData {
        driver: driver.to_string(),
        source_key: "test-source".to_string(),
        api_style: None,
        api_key: api_key.to_string(),
        base_url: base_url.map(str::to_string),
        model: ModelIdData {
            provider: "Anthropic".to_string(),
            model: model.to_string(),
        },
        max_tokens: 8192,
        requested_reasoning: ReasoningLevel::Off,
        context_window: Some(200_000),
        timeout: std::time::Duration::from_secs(60),
        user_agent: user_agent.to_string(),
    }
}

#[test]
fn model_switch_reuses_pooled_transport_for_same_endpoint_auth_driver() {
    let factory = super::provider_factory();
    let pool = factory.shared_pool().clone();

    let first = factory
        .build(spec_with(
            "claude-a",
            "anthropic",
            "sk-test-key",
            Some("https://api.anthropic.com"),
            "aemeath-test/1.0",
        ))
        .expect("first build must succeed");
    let second = factory
        .build(spec_with(
            "claude-b",
            "anthropic",
            "sk-test-key",
            Some("https://api.anthropic.com"),
            "aemeath-test/1.0",
        ))
        .expect("second build must succeed");

    assert_eq!(
        pool.distinct_transport_count(),
        1,
        "model switch must not build a second transport"
    );
    assert_ne!(
        first.provider.as_ref() as *const dyn ProviderPort,
        second.provider.as_ref() as *const dyn ProviderPort,
        "each build returns its own frozen binding port"
    );
}

#[test]
fn endpoint_driver_or_auth_change_builds_distinct_transport() {
    let factory = super::provider_factory();
    let pool = factory.shared_pool().clone();

    factory
        .build(spec_with(
            "claude-a",
            "anthropic",
            "sk-test-key",
            Some("https://api.anthropic.com"),
            "aemeath-test/1.0",
        ))
        .expect("baseline build must succeed");

    factory
        .build(spec_with(
            "claude-a",
            "anthropic",
            "sk-test-key",
            Some("https://proxy.example.com"),
            "aemeath-test/1.0",
        ))
        .expect("endpoint variant must succeed");
    assert_eq!(pool.distinct_transport_count(), 2);

    factory
        .build(spec_with(
            "claude-a",
            "anthropic",
            "sk-rotated-key",
            Some("https://api.anthropic.com"),
            "aemeath-test/1.0",
        ))
        .expect("auth variant must succeed");
    assert_eq!(pool.distinct_transport_count(), 3);

    factory
        .build(spec_with(
            "gpt-x",
            "openai",
            "sk-test-key",
            Some("https://api.openai.com"),
            "aemeath-test/1.0",
        ))
        .expect("driver variant must succeed");
    assert_eq!(pool.distinct_transport_count(), 4);
}

#[test]
fn rebuild_keeps_prior_binding_port_frozen() {
    let factory = super::provider_factory();
    let first = factory
        .build(spec_with(
            "claude-a",
            "anthropic",
            "sk-test-key",
            Some("https://api.anthropic.com"),
            "aemeath-test/1.0",
        ))
        .expect("first build must succeed");
    let first_port = std::sync::Arc::clone(&first.provider);

    factory
        .build(spec_with(
            "claude-b",
            "anthropic",
            "sk-test-key",
            Some("https://api.anthropic.com"),
            "aemeath-test/1.0",
        ))
        .expect("second build must succeed");

    assert!(
        std::sync::Arc::ptr_eq(&first_port, &first.provider),
        "rebuilding must never rewrite a prior binding's port in place"
    );
    let capability = first
        .provider
        .capabilities(&first.model)
        .expect("prior binding port must stay usable");
    assert_eq!(capability.model, first.model);
}

#[test]
fn invalid_driver_reports_configuration_error_without_pool_growth() {
    let factory = super::provider_factory();
    let pool = factory.shared_pool().clone();

    let error = factory
        .build(spec_with(
            "claude-a",
            "no-such-driver",
            "sk-test-key",
            Some("https://api.anthropic.com"),
            "aemeath-test/1.0",
        ))
        .expect_err("unknown driver must fail explicitly");

    assert_eq!(error.kind, ProviderErrorKind::Configuration);
    assert_eq!(
        pool.distinct_transport_count(),
        0,
        "failed builds must not seed pool entries"
    );
}

/// config catalog 的 driver 字符串 ⊆ share driver 身份词表：
/// catalog 新增/改名 driver 而词表未跟进时在此失败（#1850 身份敏感区，
/// 词表唯一真相源 = share::config::domain::driver_kind）。
#[test]
fn catalog_driver_strings_are_covered_by_driver_vocab() {
    for entry in config::catalog::PROVIDER_CATALOG.iter() {
        let driver_str = entry.driver.as_str();
        assert!(
            share::config::domain::driver_kind::DriverKind::parse(driver_str).is_some(),
            "catalog driver `{driver_str}` 不在身份词表内"
        );
    }
}
