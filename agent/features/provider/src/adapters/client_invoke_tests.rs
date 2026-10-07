//! `LlmClient::invoke` 转换断言组——自 composition `provider_tests.rs` 迁入。
//!
//! v2 后 `ProviderAdapter::invoke`（composition）是纯直传：system block /
//! tool schema 转换、reasoning clamp、ResolvedInvocation 校验、底层错误
//! 透传与 establishment 取消竞速全部收编进 `LlmClient::invoke`（本 crate）。
//! 原 composition 侧的转换断言失去测的对象，改在此处经 crate 内
//! `ports::LlmProvider` fake 直测 client。断言值与原测试逐一等价。

use super::LlmClient;
use crate::domain::capability::ReasoningLevel;
use crate::ports::{LlmProvider, ResolvedInvocation};
use crate::{
    InvocationDeltaData, InvocationEventData, InvocationRequestData, ModelCapabilityData,
    ModelIdData, ProviderCompletionData, ProviderContentBlockData, ProviderError,
    ProviderErrorKind, ProviderStopReasonData as StopReason, RawUsageSnapshotData,
    ReasoningCapabilityData, ReasoningMappingKindData, RequestSystemBlockData,
};
use async_trait::async_trait;
use share::message::Message;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

// ─── Captured invocation (what the fake provider received) ────────────

/// Snapshot of everything the fake provider observed in one `invocation_stream`
/// call: the resolved `ResolvedInvocation` plus the converted system blocks and
/// tool schemas. `LlmClient::invoke`'s job is to translate the provider-neutral
/// `InvocationRequestData` into these provider-domain values; the tests
/// assert that translation.
#[derive(Debug, Default, Clone)]
struct CapturedInvocation {
    scope_model: Option<String>,
    scope_max_tokens: Option<u32>,
    scope_requested_reasoning: Option<ReasoningLevel>,
    scope_effective_reasoning: Option<ReasoningLevel>,
    /// `(text, is_cacheable)` per `RequestSystemBlockData`.
    system_blocks: Vec<(String, bool)>,
    tool_schemas: Vec<serde_json::Value>,
    invocation_count: u32,
}

// ─── Minimal recording fake LlmProvider ───────────────────────────────

/// A minimal fake `LlmProvider` that records what it receives and returns a
/// fixed happy-path stream. Two knobs:
/// - `with_error`: make `invocation_stream` fail immediately with a given error.
/// - `blocking`: make `invocation_stream` await the invocation-local
///   cancellation token before returning, so a test can exercise
///   establishment-phase cancellation.
struct RecordingProvider {
    model: String,
    provider: String,
    error: Option<ProviderError>,
    captured: Arc<Mutex<CapturedInvocation>>,
    block_until_cancelled: bool,
}

impl RecordingProvider {
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

    fn with_error(mut self, err: ProviderError) -> Self {
        self.error = Some(err);
        self
    }

    /// Variant that blocks call establishment until the invocation-local
    /// cancellation token fires, simulating a slow / pending connection.
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
impl LlmProvider for RecordingProvider {
    async fn invocation_stream(
        &self,
        resolved: &ResolvedInvocation,
        system: &[RequestSystemBlockData],
        _messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStreamData, ProviderError> {
        // Record exactly what the client passed down.
        {
            let mut c = self.captured.lock().expect("captured lock poisoned");
            c.scope_model = Some(resolved.model.clone());
            c.scope_max_tokens = Some(resolved.max_tokens);
            c.scope_requested_reasoning = Some(resolved.requested_reasoning);
            c.scope_effective_reasoning = Some(resolved.effective_reasoning);
            c.system_blocks = system
                .iter()
                .map(|b| (b.text().to_string(), b.is_cacheable()))
                .collect();
            c.tool_schemas = tool_schemas.to_vec();
            c.invocation_count += 1;
        }

        if self.block_until_cancelled {
            // Simulate a pending connection: stay in establishment until the
            // invocation-local token is cancelled by the client's select bridge.
            cancel.cancelled().await;
            return Err(ProviderError::cancelled());
        }
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        Ok(Box::pin(futures_util::stream::iter(vec![
            InvocationEventData::Delta(InvocationDeltaData::Text("hello from fake".to_string())),
            InvocationEventData::Completed(ProviderCompletionData {
                output: vec![ProviderContentBlockData::Text(
                    "hello from fake".to_string(),
                )],
                stop_reason: StopReason::EndTurn,
                usage: Some(RawUsageSnapshotData {
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

fn fresh_captured() -> Arc<Mutex<CapturedInvocation>> {
    Arc::new(Mutex::new(CapturedInvocation::default()))
}

/// Build a client over a recording fake, keeping shared access to what the
/// fake observed.
fn build_client(fake: RecordingProvider) -> (Arc<LlmClient>, Arc<Mutex<CapturedInvocation>>) {
    let captured = fake.captured.clone();
    let client = Arc::new(LlmClient::from_provider(Arc::new(fake)));
    (client, captured)
}

// ─── Tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn invoke_returns_stream_with_delta_then_completed() {
    let (client, _captured) = build_client(RecordingProvider::new(
        "fake-provider",
        "fake-model",
        fresh_captured(),
    ));
    let capability = test_capability();

    let request = InvocationRequestData::new(test_model_id(), vec![], 8192, ReasoningLevel::Off);
    let cancel = CancellationToken::new();

    let mut stream = client.invoke(&capability, &request, &cancel).await.unwrap();

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
        matches!(events[0], InvocationEventData::Delta(InvocationDeltaData::Text(ref t)) if t == "hello from fake"),
        "first event should be a text delta"
    );
    assert!(
        matches!(events[1], InvocationEventData::Completed(_)),
        "second event should be Completed"
    );
}

#[tokio::test]
async fn invoke_propagates_provider_error() {
    let (client, _captured) = build_client(
        RecordingProvider::new("bad-provider", "bad-model", fresh_captured()).with_error(
            ProviderError::fatal(ProviderErrorKind::RateLimited, "too many requests"),
        ),
    );
    let capability = ModelCapabilityData {
        model: ModelIdData {
            provider: "bad-provider".to_string(),
            model: "bad-model".to_string(),
        },
        supports_tools: false,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        reasoning: ReasoningCapabilityData::none(),
        context_limit: None,
        output_limit: None,
    };

    let request =
        InvocationRequestData::new(capability.model.clone(), vec![], 8192, ReasoningLevel::Off);
    let cancel = CancellationToken::new();

    let result = client.invoke(&capability, &request, &cancel).await;
    assert!(
        matches!(result, Err(ref e) if e.kind == ProviderErrorKind::RateLimited && !e.retryable),
        "expected terminal rate-limited error"
    );
}

#[tokio::test]
async fn invoke_rejects_invalid_scope() {
    let (client, _captured) = build_client(RecordingProvider::new(
        "fake-provider",
        "fake-model",
        fresh_captured(),
    ));
    let capability = test_capability();

    // max_output_tokens = 0 should trigger a scope validation error.
    let request = InvocationRequestData::new(test_model_id(), vec![], 0, ReasoningLevel::Off);
    let cancel = CancellationToken::new();

    let result = client.invoke(&capability, &request, &cancel).await;
    assert!(
        matches!(result, Err(ref e) if e.kind == ProviderErrorKind::Configuration),
        "expected configuration error for zero max tokens"
    );
}

#[tokio::test]
async fn invoke_converts_system_blocks_tools_and_uses_neutral_scope_model() {
    let (client, captured) = build_client(RecordingProvider::new(
        "fake-provider",
        "fake-model",
        fresh_captured(),
    ));
    let capability = test_capability();

    let mut request =
        InvocationRequestData::new(test_model_id(), vec![], 8192, ReasoningLevel::Off);
    // Provider-neutral system blocks: one cacheable, one dynamic.
    request.system = vec![
        RequestSystemBlockData::Text("stable prefix first part".to_string()),
        RequestSystemBlockData::Cacheable("stable prefix boundary".to_string()),
        RequestSystemBlockData::Text("today is monday".to_string()),
    ];
    // A tool schema with full {name, description, input_schema}.
    request.tools = vec![serde_json::json!({
        "name": "get_weather",
        "description": "Get current weather",
        "input_schema": {
            "type": "object",
            "properties": { "city": { "type": "string" } },
        },
    })];

    let cancel = CancellationToken::new();
    let mut stream = client.invoke(&capability, &request, &cancel).await.unwrap();
    use futures_util::StreamExt;
    while stream.next().await.is_some() {}

    let c = captured.lock().expect("captured lock poisoned");

    // (3) Scope model is the provider-neutral model name, NOT "provider/model".
    assert_eq!(c.scope_model.as_deref(), Some("fake-model"));
    assert_ne!(c.scope_model.as_deref(), Some("fake-provider/fake-model"));
    assert_eq!(c.scope_max_tokens, Some(8192));

    // System blocks reach the provider unchanged:
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
    capability.reasoning = ReasoningCapabilityData::new(
        [ReasoningLevel::Off, ReasoningLevel::Medium],
        ReasoningMappingKindData::Effort,
    )
    .expect("valid capability");

    let (client, captured) = build_client(RecordingProvider::new(
        "fake-provider",
        "fake-model",
        fresh_captured(),
    ));

    let request = InvocationRequestData::new(test_model_id(), vec![], 4096, ReasoningLevel::Max);
    let cancel = CancellationToken::new();
    let _ = client.invoke(&capability, &request, &cancel).await.unwrap();

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
    let (client, captured) = build_client(RecordingProvider::new(
        "fake-provider",
        "fake-model",
        fresh_captured(),
    ));
    let capability = test_capability();

    let request = InvocationRequestData::new(test_model_id(), vec![], 8192, ReasoningLevel::Off);
    let cancel = CancellationToken::new();
    let mut stream = client.invoke(&capability, &request, &cancel).await.unwrap();

    use futures_util::StreamExt;
    while stream.next().await.is_some() {}

    let c = captured.lock().expect("captured lock poisoned");
    assert_eq!(
        c.invocation_count, 1,
        "a single invoke() must result in exactly one upstream invocation"
    );
}

#[tokio::test]
async fn invoke_returns_cancelled_when_signal_fires_during_establishment() {
    // A fake that parks inside call establishment until the invocation-local
    // token fires.
    let (client, _captured) = build_client(RecordingProvider::blocking(
        "fake-provider",
        "fake-model",
        fresh_captured(),
    ));
    let capability = test_capability();

    let request = InvocationRequestData::new(test_model_id(), vec![], 8192, ReasoningLevel::Off);
    let cancel = CancellationToken::new();
    let cancel_for_task = cancel.clone();
    let client_for_task = client.clone();

    // Drive invoke() on a task so we can fire the external signal mid-flight.
    let handle = tokio::spawn(async move {
        client_for_task
            .invoke(&capability, &request, &cancel_for_task)
            .await
    });

    // Let the spawned invoke reach call establishment (the fake now awaits its
    // invocation-local cancellation token).
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    cancel.cancel();

    let result = handle.await.expect("spawned invoke task panicked");
    assert!(
        matches!(result, Err(ref e) if e.is_cancelled()),
        "expected Cancelled when the external signal fires during establishment"
    );
}
