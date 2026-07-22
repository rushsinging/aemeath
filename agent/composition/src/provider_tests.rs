use super::*;
use async_trait::async_trait;
use provider::composition::{InvocationScope, LlmClient, LlmProvider, SystemBlock};
use provider::{
    InvocationDelta, InvocationEvent, InvocationOptions, InvocationRequest, ModelCapability,
    ModelId, ModelToolSchema, ProviderCompletion, ProviderContentBlock, ProviderErrorKind,
    ProviderStopReason as StopReason, RawUsageSnapshot, ReasoningCapability, ReasoningLevel,
    ReasoningMappingKind,
};
use share::message::Message;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

// ─── Captured invocation (what the fake provider received) ────────────

/// Snapshot of everything the fake provider observed in one `invocation_stream`
/// call: the resolved `InvocationScope` plus the converted system blocks and
/// tool schemas. The adapter's job is to translate the provider-neutral
/// `InvocationRequest` into these legacy provider-domain values; the tests
/// assert that translation.
#[derive(Debug, Default, Clone)]
struct CapturedInvocation {
    scope_model: Option<String>,
    scope_max_tokens: Option<u32>,
    scope_requested_reasoning: Option<ReasoningLevel>,
    scope_effective_reasoning: Option<ReasoningLevel>,
    /// `(text, is_cacheable)` per legacy `SystemBlock`.
    system_blocks: Vec<(String, bool)>,
    tool_schemas: Vec<serde_json::Value>,
    invocation_count: u32,
}

// ─── Minimal fake LlmProvider ─────────────────────────────────────────

/// A minimal fake `LlmProvider` that records what it receives and returns a
/// fixed happy-path stream. Two knobs:
/// - `with_error`: make `invocation_stream` fail immediately with a given error.
/// - `blocking`: make `invocation_stream` await the local cancellation token
///   before returning, so a test can exercise establishment-phase cancellation.
struct FakeLlmProvider {
    model: String,
    provider: String,
    error: Option<provider::ProviderError>,
    captured: Arc<Mutex<CapturedInvocation>>,
    block_until_cancelled: bool,
}

impl FakeLlmProvider {
    fn new(
        provider_name: &str,
        model_name: &str,
        captured: Arc<Mutex<CapturedInvocation>>,
    ) -> Self {
        Self {
            model: model_name.to_string(),
            provider: provider_name.to_string(),
            error: None,
            captured,
            block_until_cancelled: false,
        }
    }

    fn with_error(mut self, err: provider::ProviderError) -> Self {
        self.error = Some(err);
        self
    }

    /// Variant that blocks call establishment until the (local) cancellation
    /// token fires, simulating a slow / pending connection.
    fn blocking(
        provider_name: &str,
        model_name: &str,
        captured: Arc<Mutex<CapturedInvocation>>,
    ) -> Self {
        let mut this = Self::new(provider_name, model_name, captured);
        this.block_until_cancelled = true;
        this
    }
}

#[async_trait]
impl LlmProvider for FakeLlmProvider {
    async fn invocation_stream(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        _messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<provider::InvocationStream, provider::ProviderError> {
        // Record exactly what the adapter passed down.
        {
            let mut c = self.captured.lock().expect("captured lock poisoned");
            c.scope_model = Some(scope.model().to_string());
            c.scope_max_tokens = Some(scope.max_tokens());
            c.scope_requested_reasoning = Some(scope.requested_reasoning());
            c.scope_effective_reasoning = Some(scope.effective_reasoning());
            c.system_blocks = system
                .iter()
                .map(|b| (b.text.clone(), b.cache_control.is_some()))
                .collect();
            c.tool_schemas = tool_schemas.to_vec();
            c.invocation_count += 1;
        }

        if self.block_until_cancelled {
            // Simulate a pending connection: stay in establishment until the
            // local token is cancelled by the adapter's select bridge.
            cancel.cancelled().await;
            return Err(provider::ProviderError::cancelled());
        }
        if cancel.is_cancelled() {
            return Err(provider::ProviderError::cancelled());
        }
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        Ok(Box::pin(futures_util::stream::iter(vec![
            InvocationEvent::Delta(InvocationDelta::Text("hello from fake".to_string())),
            InvocationEvent::Completed(ProviderCompletion {
                output: vec![ProviderContentBlock::Text("hello from fake".to_string())],
                stop_reason: StopReason::EndTurn,
                usage: Some(RawUsageSnapshot {
                    input_tokens: Some(5),
                    output_tokens: Some(3),
                    ..Default::default()
                }),
                effective_reasoning: ReasoningLevel::Off,
            }),
        ])))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.provider
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────

fn test_model_id() -> ModelId {
    ModelId {
        provider: "fake-provider".to_string(),
        model: "fake-model".to_string(),
    }
}

fn test_capability() -> ModelCapability {
    ModelCapability {
        model: test_model_id(),
        supports_tools: true,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        reasoning: ReasoningCapability::none(),
        context_limit: Some(128_000),
        output_limit: Some(8_192),
    }
}

fn fresh_captured() -> Arc<Mutex<CapturedInvocation>> {
    Arc::new(Mutex::new(CapturedInvocation::default()))
}

/// Build a port over a recording fake provider and the given capability,
/// returning shared access to what the fake observed.
fn build_port_with_capability(
    capability: ModelCapability,
) -> (
    Arc<dyn ProviderPort>,
    ModelId,
    Arc<Mutex<CapturedInvocation>>,
) {
    let captured = fresh_captured();
    let model = capability.model.clone();
    let fake = Arc::new(FakeLlmProvider::new(
        &model.provider,
        &model.model,
        captured.clone(),
    ));
    let client = Arc::new(LlmClient::from_provider(fake));
    let caps = HashMap::from([(model.clone(), capability)]);
    (provider_port(client, caps), model, captured)
}

/// Build a port whose fake records into a shared snapshot.
fn build_port_capturing() -> (
    Arc<dyn ProviderPort>,
    ModelId,
    Arc<Mutex<CapturedInvocation>>,
) {
    build_port_with_capability(test_capability())
}

/// Build a port for tests that don't inspect what the fake received.
fn build_port() -> (Arc<dyn ProviderPort>, ModelId) {
    let (port, model, _captured) = build_port_capturing();
    (port, model)
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

    let unknown = ModelId {
        provider: "unknown".to_string(),
        model: "x".to_string(),
    };
    let err = port.capabilities(&unknown).unwrap_err();
    assert_eq!(err.kind, ProviderErrorKind::ModelUnavailable);
    assert!(!err.retryable);
}

#[tokio::test]
async fn invoke_returns_stream_with_delta_then_completed() {
    let (port, model) = build_port();

    let request = InvocationRequest::new(
        model,
        vec![],
        InvocationOptions::new(8192, ReasoningLevel::Off),
    );
    let cancel = CancellationToken::new();

    let mut stream = port.invoke(request, &cancel).await.unwrap();

    use futures_util::StreamExt;

    let mut events = Vec::new();
    while let Some(evt) = stream.next().await {
        events.push(evt);
    }

    assert_eq!(
        events.len(),
        2,
        "expected exactly 2 events: Delta + Completed"
    );
    assert!(
        matches!(events[0], InvocationEvent::Delta(InvocationDelta::Text(ref t)) if t == "hello from fake"),
        "first event should be a text delta"
    );
    assert!(
        matches!(events[1], InvocationEvent::Completed(_)),
        "second event should be Completed"
    );
}

#[tokio::test]
async fn invoke_returns_cancelled_when_signal_already_set() {
    let (port, model) = build_port();

    let request = InvocationRequest::new(
        model,
        vec![],
        InvocationOptions::new(8192, ReasoningLevel::Off),
    );
    let cancel = CancellationToken::new();
    cancel.cancel();

    let result = port.invoke(request, &cancel).await;
    assert!(
        matches!(result, Err(ref e) if e.is_cancelled()),
        "expected cancelled error"
    );
}

#[tokio::test]
async fn invoke_propagates_provider_error() {
    let captured = fresh_captured();
    let model = ModelId {
        provider: "bad-provider".to_string(),
        model: "bad-model".to_string(),
    };
    let fake = Arc::new(
        FakeLlmProvider::new("bad-provider", "bad-model", captured).with_error(
            provider::ProviderError::retryable(
                provider::ProviderErrorKind::RateLimited,
                "too many requests",
            ),
        ),
    );
    let client = Arc::new(LlmClient::from_provider(fake));
    let capability = ModelCapability {
        model: model.clone(),
        supports_tools: false,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        reasoning: ReasoningCapability::none(),
        context_limit: None,
        output_limit: None,
    };
    let port = provider_port(client, HashMap::from([(model.clone(), capability)]));

    let request = InvocationRequest::new(
        model,
        vec![],
        InvocationOptions::new(8192, ReasoningLevel::Off),
    );
    let cancel = CancellationToken::new();

    let result = port.invoke(request, &cancel).await;
    assert!(
        matches!(result, Err(ref e) if e.kind == provider::ProviderErrorKind::RateLimited && e.retryable),
        "expected rate-limited error"
    );
}

#[tokio::test]
async fn invoke_rejects_invalid_scope() {
    let (port, model) = build_port();

    // max_output_tokens = 0 should trigger a scope validation error.
    let request = InvocationRequest::new(
        model,
        vec![],
        InvocationOptions::new(0, ReasoningLevel::Off),
    );
    let cancel = CancellationToken::new();

    let result = port.invoke(request, &cancel).await;
    assert!(
        matches!(result, Err(ref e) if e.kind == ProviderErrorKind::Configuration),
        "expected configuration error for zero max tokens"
    );
}

// ─── New: translation / clamp / single-invocation / cancel ────────────

#[tokio::test]
async fn invoke_converts_system_blocks_tools_and_uses_neutral_scope_model() {
    let (port, model, captured) = build_port_capturing();

    let mut request = InvocationRequest::new(
        model,
        vec![],
        InvocationOptions::new(8192, ReasoningLevel::Off),
    );
    // Provider-neutral system blocks: one cacheable, one dynamic.
    request.system = vec![
        provider::RequestSystemBlock::Text("stable prefix first part".to_string()),
        provider::RequestSystemBlock::Cacheable("stable prefix boundary".to_string()),
        provider::RequestSystemBlock::Text("today is monday".to_string()),
    ];
    // A tool schema with full {name, description, input_schema}.
    request.tools = vec![ModelToolSchema {
        name: "get_weather".to_string(),
        description: "Get current weather".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
        }),
    }];

    let cancel = CancellationToken::new();
    let mut stream = port.invoke(request, &cancel).await.unwrap();
    use futures_util::StreamExt;
    while stream.next().await.is_some() {}

    let c = captured.lock().expect("captured lock poisoned");

    // (3) Scope model is the provider-neutral model name, NOT "provider/model".
    assert_eq!(c.scope_model.as_deref(), Some("fake-model"));
    assert_ne!(c.scope_model.as_deref(), Some("fake-provider/fake-model"));
    assert_eq!(c.scope_max_tokens, Some(8192));

    // System blocks converted at the Provider composition boundary:
    //     Cacheable → cache_control present (ephemeral), Text → absent.
    assert_eq!(
        c.system_blocks,
        vec![
            ("stable prefix first part".to_string(), false),
            ("stable prefix boundary".to_string(), true),
            ("today is monday".to_string(), false),
        ]
    );

    // (2) Tool schema converted to a complete JSON object
    //     {name, description, input_schema}, not the bare input_schema.
    assert_eq!(c.tool_schemas.len(), 1);
    let tool = &c.tool_schemas[0];
    assert_eq!(tool["name"], "get_weather");
    assert_eq!(tool["description"], "Get current weather");
    assert_eq!(tool["input_schema"]["properties"]["city"]["type"], "string");
}

#[tokio::test]
async fn invoke_clamps_requested_reasoning_to_capability() {
    // Capability supports only Off and Medium; requesting Max must clamp to Medium.
    let mut capability = test_capability();
    capability.reasoning = ReasoningCapability::new(
        [ReasoningLevel::Off, ReasoningLevel::Medium],
        ReasoningMappingKind::Effort,
    )
    .expect("valid capability");

    let (port, model, captured) = build_port_with_capability(capability);

    let request = InvocationRequest::new(
        model,
        vec![],
        InvocationOptions::new(4096, ReasoningLevel::Max),
    );
    let cancel = CancellationToken::new();
    let _ = port.invoke(request, &cancel).await.unwrap();

    let c = captured.lock().expect("captured lock poisoned");
    assert_eq!(
        c.scope_requested_reasoning,
        Some(ReasoningLevel::Max),
        "requested reasoning is preserved verbatim"
    );
    assert_eq!(
        c.scope_effective_reasoning,
        Some(ReasoningLevel::Medium),
        "effective reasoning is clamped to the capability maximum"
    );
    assert!(
        c.scope_effective_reasoning.unwrap() <= c.scope_requested_reasoning.unwrap(),
        "effective must not exceed requested"
    );
}

#[tokio::test]
async fn invoke_invokes_provider_exactly_once() {
    let (port, model, captured) = build_port_capturing();

    let request = InvocationRequest::new(
        model,
        vec![],
        InvocationOptions::new(8192, ReasoningLevel::Off),
    );
    let cancel = CancellationToken::new();
    let mut stream = port.invoke(request, &cancel).await.unwrap();

    use futures_util::StreamExt;
    while stream.next().await.is_some() {}

    let c = captured.lock().expect("captured lock poisoned");
    assert_eq!(
        c.invocation_count, 1,
        "a single invoke() must result in exactly one upstream invocation"
    );
}

#[tokio::test]
async fn invoke_rejects_unknown_model() {
    let (port, _known) = build_port();

    let unknown = ModelId {
        provider: "nope".to_string(),
        model: "ghost".to_string(),
    };
    let request = InvocationRequest::new(
        unknown,
        vec![],
        InvocationOptions::new(8192, ReasoningLevel::Off),
    );
    let cancel = CancellationToken::new();

    let result = port.invoke(request, &cancel).await;
    assert!(
        matches!(result, Err(ref e) if e.kind == ProviderErrorKind::ModelUnavailable),
        "expected ModelUnavailable for a model with no declared capability"
    );
}

#[tokio::test]
async fn invoke_returns_cancelled_when_signal_fires_during_establishment() {
    // A fake that parks inside call establishment until its local token fires.
    let captured = fresh_captured();
    let model = test_model_id();
    let fake = Arc::new(FakeLlmProvider::blocking(
        "fake-provider",
        "fake-model",
        captured,
    ));
    let client = Arc::new(LlmClient::from_provider(fake));
    let port: Arc<dyn ProviderPort> =
        provider_port(client, HashMap::from([(model.clone(), test_capability())]));

    let cancel = CancellationToken::new();
    let cancel_for_task = cancel.clone();
    let port_for_task = port.clone();

    // Drive invoke() on a task so we can fire the external signal mid-flight.
    let handle = tokio::spawn(async move {
        let request = InvocationRequest::new(
            model,
            vec![],
            InvocationOptions::new(8192, ReasoningLevel::Off),
        );
        port_for_task.invoke(request, &cancel_for_task).await
    });

    // Let the spawned invoke reach call establishment (the fake now awaits its
    // local cancellation token).
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    cancel.cancel();

    let result = handle.await.expect("spawned invoke task panicked");
    assert!(
        matches!(result, Err(ref e) if e.is_cancelled()),
        "expected Cancelled when the external signal fires during establishment"
    );
}

// ─── ProviderFactory TDD tests ─────────────────────────────────────

use runtime::ProviderBuildSpec;

fn valid_spec() -> ProviderBuildSpec {
    ProviderBuildSpec {
        driver: "anthropic".to_string(),
        source_key: "test-source".to_string(),
        api_style: None,
        api_key: "sk-test-key".to_string(),
        base_url: None,
        model: ModelId {
            provider: "Anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
        },
        max_tokens: 8192,
        requested_reasoning: ReasoningLevel::Off,
        context_window: Some(200_000),
        timeout: std::time::Duration::from_secs(60),
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
    spec.model = ModelId {
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
