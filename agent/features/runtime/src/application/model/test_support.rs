use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream;
use provider::{
    InvocationDeltaData, InvocationEventData, InvocationStreamData, ProviderCompletionData,
    ProviderContentBlockData, ProviderError, ProviderStopReasonData, RawUsageSnapshotData,
};
use share::reasoning::ReasoningLevel;

pub(crate) fn text_completion_stream(
    text: impl Into<String>,
    input_tokens: u32,
    output_tokens: u32,
) -> InvocationStreamData {
    let text = text.into();
    Box::pin(stream::iter([
        InvocationEventData::Delta(InvocationDeltaData::Text(text.clone())),
        InvocationEventData::Completed(ProviderCompletionData {
            output: vec![ProviderContentBlockData::Text(text)],
            stop_reason: ProviderStopReasonData::EndTurn,
            usage: Some(RawUsageSnapshotData {
                input_tokens: Some(input_tokens),
                output_tokens: Some(output_tokens),
                ..RawUsageSnapshotData::default()
            }),
            effective_reasoning: ReasoningLevel::Off,
        }),
    ]))
}

#[derive(Clone)]
pub(crate) struct ScriptedInvocationProvider {
    attempts: Arc<Mutex<VecDeque<Vec<InvocationEventData>>>>,
    calls: Arc<Mutex<usize>>,
}

impl ScriptedInvocationProvider {
    pub(crate) fn new(attempts: Vec<Vec<InvocationEventData>>) -> Self {
        Self {
            attempts: Arc::new(Mutex::new(VecDeque::from(attempts))),
            calls: Arc::new(Mutex::new(0)),
        }
    }

    pub(crate) fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

#[async_trait]
impl provider::composition::LlmProvider for ScriptedInvocationProvider {
    async fn invocation_stream(
        &self,
        _scope: &provider::composition::InvocationScopeData,
        _system: &[provider::composition::SystemBlockData],
        _messages: &[share::message::Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<InvocationStreamData, ProviderError> {
        *self.calls.lock().unwrap() += 1;
        let events = self
            .attempts
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted invocation provider attempt");
        Ok(Box::pin(futures::stream::iter(events)))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

pub(crate) fn empty_completion() -> InvocationEventData {
    InvocationEventData::Completed(ProviderCompletionData {
        output: Vec::new(),
        stop_reason: ProviderStopReasonData::EndTurn,
        usage: Some(RawUsageSnapshotData::default()),
        effective_reasoning: ReasoningLevel::Off,
    })
}

pub(crate) fn successful_completion(text: &str) -> InvocationEventData {
    InvocationEventData::Completed(ProviderCompletionData {
        output: vec![ProviderContentBlockData::Text(text.to_string())],
        stop_reason: ProviderStopReasonData::EndTurn,
        usage: Some(RawUsageSnapshotData::default()),
        effective_reasoning: ReasoningLevel::Off,
    })
}

pub(crate) const RETRY_ADVANCE_LIMITS: [std::time::Duration; 10] = [
    std::time::Duration::from_secs(11),
    std::time::Duration::from_secs(21),
    std::time::Duration::from_secs(41),
    std::time::Duration::from_secs(81),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
    std::time::Duration::from_secs(121),
];

pub(crate) async fn advance_until_retry_condition(
    description: &str,
    virtual_time_limit: std::time::Duration,
    condition: impl Fn() -> bool,
) {
    // Under the full parallel Runtime suite, Main Run setup can require many
    // scheduler turns before it registers the retry timer. Advancing paused
    // time during that setup would consume the virtual-time budget before the
    // timer exists and make the test fail even though retry behavior is sound.
    for _ in 0..10_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }

    let tick = std::time::Duration::from_millis(100);
    let max_ticks = virtual_time_limit.as_millis().div_ceil(tick.as_millis());
    for _ in 0..max_ticks {
        if condition() {
            return;
        }
        tokio::time::advance(tick).await;
    }
    tokio::task::yield_now().await;
    assert!(condition(), "timed out waiting for {description}");
}

// ─── Test ProviderPort helpers (#907) ────────────────────────────

/// Per-call custom invocation hook for `TestProviderPort` (#907 loop test migration).
///
/// Receives `(call_index, request, cancellation)` and must return the future of
/// the resulting invocation stream (or error). When set via `with_invocation_fn`,
/// it **fully overrides** the default `error → blocking → cancel → responses-queue`
/// dispatch. Tests use this to keep `Sequence`/`recording`/`error`/`cancel` behavior
/// without writing bespoke provider port impls.
///
/// Uses `for<'a>` HRTB so closures can capture the borrowed `&InvocationRequestData`
/// / `&dyn CancellationSignal` into their returned `Future + 'a`.
pub(crate) type TestInvocationFn = Arc<
    dyn for<'a> Fn(
            usize,
            &'a crate::ports::provider_port::InvocationRequestData,
            &'a dyn crate::ports::provider_port::CancellationSignal,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            crate::ports::provider_port::InvocationStreamData,
                            crate::ports::provider_port::ProviderError,
                        >,
                    > + Send
                    + 'a,
            >,
        > + Send
        + Sync,
>;

/// A programmable `ProviderPort` for tests.
pub(crate) struct TestProviderPort {
    pub responses: Arc<Mutex<VecDeque<String>>>,
    pub error: Option<crate::ports::provider_port::ProviderError>,
    pub model: provider::ModelIdData,
    pub blocking: bool,
    pub seen: Option<Arc<Mutex<Vec<::logging::LogContext>>>>,
    pub calls: Arc<Mutex<usize>>,
    /// Optional per-call hook overriding default dispatch (see [`TestInvocationFn`]).
    pub invocation_fn: Option<TestInvocationFn>,
}

impl TestProviderPort {
    pub fn new(responses: Vec<&str>, model: provider::ModelIdData) -> Self {
        Self {
            responses: Arc::new(Mutex::new(
                responses.into_iter().map(str::to_string).collect(),
            )),
            error: None,
            model,
            blocking: false,
            seen: None,
            calls: Arc::new(Mutex::new(0)),
            invocation_fn: None,
        }
    }

    /// Install a per-call invocation hook that overrides default behavior.
    pub fn with_invocation_fn(mut self, f: TestInvocationFn) -> Self {
        self.invocation_fn = Some(f);
        self
    }
}

#[async_trait]
impl crate::ports::ProviderPort for TestProviderPort {
    fn capabilities(
        &self,
        model: &provider::ModelIdData,
    ) -> Result<
        crate::ports::provider_port::ModelCapabilityData,
        crate::ports::provider_port::ProviderError,
    > {
        use crate::ports::provider_port::{
            ModelCapabilityData, ProviderError, ProviderErrorKind, ReasoningCapabilityData,
        };
        if model == &self.model {
            Ok(ModelCapabilityData {
                model: model.clone(),
                supports_tools: true,
                supports_parallel_tool_calls: true,
                supports_streaming: true,
                reasoning: ReasoningCapabilityData::none(),
                context_limit: Some(128_000),
                output_limit: Some(8192),
            })
        } else {
            Err(ProviderError::fatal(
                ProviderErrorKind::ModelUnavailable,
                format!("unknown model: {model}"),
            ))
        }
    }

    async fn invoke(
        &self,
        request: crate::ports::provider_port::InvocationRequestData,
        cancellation: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::InvocationStreamData,
        crate::ports::provider_port::ProviderError,
    > {
        use crate::ports::provider_port::ProviderError;
        let call_index = {
            let mut guard = self.calls.lock().unwrap();
            let idx = *guard;
            *guard += 1;
            idx
        };
        if let Some(ref seen) = self.seen {
            seen.lock().unwrap().push(::logging::capture());
        }
        // Custom invocation hook overrides default dispatch.
        if let Some(ref f) = self.invocation_fn {
            return f(call_index, &request, cancellation).await;
        }
        if let Some(ref e) = self.error {
            return Err(e.clone());
        }
        if self.blocking {
            cancellation.cancelled().await;
            return Err(ProviderError::cancelled());
        }
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        let text = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "fallback final response".to_string());
        Ok(text_completion_stream(text, 1, 1))
    }
}

pub(crate) fn test_binding(responses: Vec<&str>) -> Arc<crate::ports::ProviderBinding> {
    let model_id = test_model_id();
    let port = Arc::new(TestProviderPort::new(responses, model_id.clone()));
    Arc::new(crate::ports::ProviderBinding {
        provider: port,
        model: model_id,
        max_tokens: 8192,
        requested_reasoning: crate::ports::provider_port::ReasoningLevel::Off,
        context_window: Some(128_000),
    })
}

pub(crate) fn test_binding_from_port(port: TestProviderPort) -> Arc<crate::ports::ProviderBinding> {
    let model_id = port.model.clone();
    Arc::new(crate::ports::ProviderBinding {
        provider: Arc::new(port),
        model: model_id,
        max_tokens: 8192,
        requested_reasoning: crate::ports::provider_port::ReasoningLevel::Off,
        context_window: Some(128_000),
    })
}

/// Default `ModelIdData` used by `test_binding*` helpers.
pub(crate) fn test_model_id() -> provider::ModelIdData {
    provider::ModelIdData {
        provider: "test".to_string(),
        model: "test-model".to_string(),
    }
}

// ─── Test ProviderFactory (#907) ──────────────────────────────

/// A `ProviderFactory` that always returns the same binding for any spec.
///
/// Used by sub-agent runner tests where the binding's `ProviderPort` (e.g.
/// `TestProviderPort`) is what we want exercised, regardless of how the
/// runner resolved the `ProviderBuildSpec` from `ModelsConfig`.
pub(crate) struct ConstantTestFactory {
    binding: Arc<crate::ports::ProviderBinding>,
}

impl ConstantTestFactory {
    pub fn new(binding: Arc<crate::ports::ProviderBinding>) -> Self {
        Self { binding }
    }
}

impl crate::ports::ProviderFactory for ConstantTestFactory {
    fn build(
        &self,
        _spec: crate::ports::ProviderBuildSpec,
    ) -> Result<crate::ports::ProviderBinding, crate::ports::provider_port::ProviderError> {
        Ok(self.binding.as_ref().clone())
    }
}

pub(crate) fn constant_factory(
    binding: Arc<crate::ports::ProviderBinding>,
) -> Arc<dyn crate::ports::ProviderFactory> {
    Arc::new(ConstantTestFactory::new(binding))
}

// ─── LlmProvider → ProviderPort adapter (#907 loop test migration) ────────

/// Adapter that implements [`crate::ports::ProviderPort`] by delegating to an
/// existing `provider::composition::LlmProvider` scripted fake.
///
/// Used only by `runtime` lib tests as a minimal bridge so the legacy scripted
/// fakes (e.g. `SequenceProvider`, `RecordingProvider`, `CountingProvider`,
/// `ErrorProvider`) can be wrapped in a `ProviderBinding` without rewriting
/// every test to the new `ProviderPort` trait.
struct LlmProviderPortAdapter {
    provider: std::sync::Arc<dyn provider::composition::LlmProvider>,
    model: provider::ModelIdData,
}

impl LlmProviderPortAdapter {
    fn new(provider: std::sync::Arc<dyn provider::composition::LlmProvider>) -> Self {
        let model = provider::ModelIdData {
            provider: provider.provider_name().to_string(),
            model: provider.model_name().to_string(),
        };
        Self { provider, model }
    }
}

#[async_trait]
impl crate::ports::ProviderPort for LlmProviderPortAdapter {
    fn capabilities(
        &self,
        model: &provider::ModelIdData,
    ) -> Result<
        crate::ports::provider_port::ModelCapabilityData,
        crate::ports::provider_port::ProviderError,
    > {
        use crate::ports::provider_port::{
            ModelCapabilityData, ProviderError, ProviderErrorKind, ReasoningCapabilityData,
        };
        if model == &self.model {
            Ok(ModelCapabilityData {
                model: model.clone(),
                supports_tools: true,
                supports_parallel_tool_calls: true,
                supports_streaming: true,
                reasoning: ReasoningCapabilityData::none(),
                context_limit: Some(128_000),
                output_limit: Some(8_192),
            })
        } else {
            Err(ProviderError::fatal(
                ProviderErrorKind::ModelUnavailable,
                format!("unknown model: {model}"),
            ))
        }
    }

    async fn invoke(
        &self,
        request: crate::ports::provider_port::InvocationRequestData,
        cancellation: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::InvocationStreamData,
        crate::ports::provider_port::ProviderError,
    > {
        // Convert InvocationRequestData into the legacy LlmProvider argument list.
        let system_blocks: Vec<provider::composition::SystemBlockData> = request
            .system
            .iter()
            .map(|block| provider::composition::SystemBlockData::dynamic(block.text().to_string()))
            .collect();
        let tool_schemas: Vec<serde_json::Value> = request
            .tools
            .iter()
            .map(|tool| tool.to_tool_definition())
            .collect();
        // Forward the cancellation token that the request carries; the
        // `CancellationSignal` arg from ProviderPort::invoke is treated as
        // advisory (real cancellation originates from `request.cancellation`).
        let _ = cancellation;
        let scope = provider::composition::InvocationScopeData::new(
            self.model.model.clone(),
            request.options.max_output_tokens.max(1),
            share::reasoning::ReasoningLevel::Off,
            share::reasoning::ReasoningLevel::Off,
        )
        .map_err(|error| {
            crate::ports::provider_port::ProviderError::fatal(
                crate::ports::provider_port::ProviderErrorKind::Configuration,
                format!("invalid invocation scope: {error}"),
            )
        })?;
        self.provider
            .invocation_stream(
                &scope,
                &system_blocks,
                &request.messages,
                &tool_schemas,
                &request.cancellation,
            )
            .await
    }
}

/// Wrap an existing `provider::composition::LlmProvider` scripted fake into a
/// `ProviderBinding` so session-driver and agent tests can reuse their scripted
/// providers without rewriting the fake bodies.
///
/// The binding's `model`/`max_tokens`/`context_window` mirror the values used by
/// the script fakes' default `LlmClient::from_provider(...)` construction.
pub(crate) fn binding_from_llm_provider(
    provider: std::sync::Arc<dyn provider::composition::LlmProvider>,
) -> std::sync::Arc<crate::ports::ProviderBinding> {
    let model = provider::ModelIdData {
        provider: provider.provider_name().to_string(),
        model: provider.model_name().to_string(),
    };
    std::sync::Arc::new(crate::ports::ProviderBinding {
        provider: std::sync::Arc::new(LlmProviderPortAdapter::new(provider)),
        model,
        max_tokens: 8192,
        requested_reasoning: crate::ports::provider_port::ReasoningLevel::Off,
        context_window: Some(128_000),
    })
}
