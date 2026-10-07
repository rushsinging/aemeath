use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream;
use provider::{
    ProviderContentData, ProviderError, ProviderResponseChunk, ProviderResponseStream,
    ProviderStopReasonData, TokenUsageData,
};

pub(crate) fn text_completion_stream(
    text: impl Into<String>,
    input_tokens: u32,
    output_tokens: u32,
) -> ProviderResponseStream {
    let text = text.into();
    Box::pin(stream::iter([
        ProviderResponseChunk::Content(ProviderContentData::Text(text)),
        ProviderResponseChunk::Usage(TokenUsageData {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            ..TokenUsageData::default()
        }),
        ProviderResponseChunk::Stop(ProviderStopReasonData::EndTurn),
    ]))
}

#[derive(Clone)]
pub(crate) struct ScriptedInvocationProvider {
    attempts: Arc<Mutex<VecDeque<Vec<ProviderResponseChunk>>>>,
    calls: Arc<Mutex<usize>>,
}

impl ScriptedInvocationProvider {
    pub(crate) fn new(attempts: Vec<Vec<ProviderResponseChunk>>) -> Self {
        Self {
            attempts: Arc::new(Mutex::new(VecDeque::from(attempts))),
            calls: Arc::new(Mutex::new(0)),
        }
    }

    pub(crate) fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

/// runtime 测试自有的 scripted provider 契约（#1861 C12c：不再借用
/// provider 内部 driver trait——fake 基建自产事件流）。
///
/// 消费完整 request（messages/system/tools/max_tokens 均可断言），
/// 返回预设事件流；取消经 `request.cancellation` 表达。
#[async_trait]
pub(crate) trait ScriptedLlmProvider: Send + Sync {
    async fn scripted_invocation_stream(
        &self,
        request: &crate::ports::provider_port::InvocationRequestData,
    ) -> Result<ProviderResponseStream, ProviderError>;

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

#[async_trait]
impl ScriptedLlmProvider for ScriptedInvocationProvider {
    async fn scripted_invocation_stream(
        &self,
        _request: &crate::ports::provider_port::InvocationRequestData,
    ) -> Result<ProviderResponseStream, ProviderError> {
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

/// 空输出成功闭合帧序列：仅 `Stop` 终止帧（`usage: None` 等价 default）。
pub(crate) fn empty_completion() -> Vec<ProviderResponseChunk> {
    vec![ProviderResponseChunk::Stop(ProviderStopReasonData::EndTurn)]
}

/// 文本成功闭合帧序列：`Content(Text)` + `Stop`。
pub(crate) fn successful_completion(text: &str) -> Vec<ProviderResponseChunk> {
    vec![
        ProviderResponseChunk::Content(ProviderContentData::Text(text.to_string())),
        ProviderResponseChunk::Stop(ProviderStopReasonData::EndTurn),
    ]
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
                            crate::ports::provider_port::ProviderResponseStream,
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
    pub model: provider::ModelInfo,
    pub blocking: bool,
    pub seen: Option<Arc<Mutex<Vec<::logging::LogContext>>>>,
    pub calls: Arc<Mutex<usize>>,
    /// Optional per-call hook overriding default dispatch (see [`TestInvocationFn`]).
    pub invocation_fn: Option<TestInvocationFn>,
}

impl TestProviderPort {
    pub fn new(responses: Vec<&str>, model: provider::ModelInfo) -> Self {
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
    // `capabilities()` 已删除（#1880）：fake 与生产一致——binding 持全量
    // ModelInfo，运行时零查询（unknown model 门禁在装配时）。

    async fn invoke(
        &self,
        request: crate::ports::provider_port::InvocationRequestData,
        cancellation: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::ProviderResponseStream,
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

pub(crate) fn test_binding(responses: Vec<&str>) -> Arc<crate::ports::ProviderBindingData> {
    let model = test_model_info();
    let port = Arc::new(TestProviderPort::new(responses, model.clone()));
    Arc::new(crate::ports::ProviderBindingData {
        provider: port,
        model,
        max_tokens: 8192,
        requested_reasoning: crate::ports::provider_port::ReasoningLevel::Off,
    })
}

pub(crate) fn test_binding_from_port(
    port: TestProviderPort,
) -> Arc<crate::ports::ProviderBindingData> {
    let model = port.model.clone();
    Arc::new(crate::ports::ProviderBindingData {
        provider: Arc::new(port),
        model,
        max_tokens: 8192,
        requested_reasoning: crate::ports::provider_port::ReasoningLevel::Off,
    })
}

/// Default `ModelInfo` used by `test_binding*` helpers.
pub(crate) fn test_model_info() -> provider::ModelInfo {
    provider::ModelInfo {
        provider: "test".to_string(),
        model: "test-model".to_string(),
        supports_tools: true,
        supports_parallel_tool_calls: true,
        supports_streaming: true,
        supported_reasoning: vec![share::reasoning::ReasoningLevel::Off],
        context_limit: Some(128_000),
        output_limit: Some(8192),
    }
}

// ─── Test ProviderFactory (#907) ──────────────────────────────

/// A `ProviderFactory` that always returns the same binding for any spec.
///
/// Used by sub-agent runner tests where the binding's `ProviderPort` (e.g.
/// `TestProviderPort`) is what we want exercised, regardless of how the
/// runner resolved the `ProviderBuildSpecData` from `ModelsConfig`.
pub(crate) struct ConstantTestFactory {
    binding: Arc<crate::ports::ProviderBindingData>,
}

impl ConstantTestFactory {
    pub fn new(binding: Arc<crate::ports::ProviderBindingData>) -> Self {
        Self { binding }
    }
}

impl crate::ports::ProviderFactory for ConstantTestFactory {
    fn build(
        &self,
        _spec: crate::ports::ProviderBuildSpecData,
    ) -> Result<crate::ports::ProviderBindingData, crate::ports::provider_port::ProviderError> {
        Ok(self.binding.as_ref().clone())
    }
}

pub(crate) fn constant_factory(
    binding: Arc<crate::ports::ProviderBindingData>,
) -> Arc<dyn crate::ports::ProviderFactory> {
    Arc::new(ConstantTestFactory::new(binding))
}

// ─── ScriptedLlmProvider → ProviderPort adapter (#907 loop test migration) ────

/// Adapter that implements [`crate::ports::ProviderPort`] by delegating to an
/// existing runtime-local [`ScriptedLlmProvider`] scripted fake.
///
/// Used only by `runtime` lib tests as a minimal bridge so the scripted
/// fakes (e.g. `SequenceProvider`, `RecordingProvider`, `CountingProvider`,
/// `ErrorProvider`) can be wrapped in a `ProviderBindingData` without rewriting
/// every test to the new `ProviderPort` trait.
struct ScriptedProviderPortAdapter {
    provider: std::sync::Arc<dyn ScriptedLlmProvider>,
}

impl ScriptedProviderPortAdapter {
    fn new(provider: std::sync::Arc<dyn ScriptedLlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl crate::ports::ProviderPort for ScriptedProviderPortAdapter {
    async fn invoke(
        &self,
        request: crate::ports::provider_port::InvocationRequestData,
        cancellation: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::ProviderResponseStream,
        crate::ports::provider_port::ProviderError,
    > {
        // 取消语义经 request.cancellation 表达（advisory 信号忽略）。
        let _ = cancellation;
        self.provider.scripted_invocation_stream(&request).await
    }
}

/// Wrap an existing runtime-local [`ScriptedLlmProvider`] scripted fake into a
/// `ProviderBindingData` so session-driver and agent tests can reuse their scripted
/// providers without rewriting the fake bodies.
///
/// The binding's `model`/`max_tokens` mirror the values used by
/// the script fakes' default `LlmClient::from_provider(...)` construction.
pub(crate) fn binding_from_llm_provider(
    provider: std::sync::Arc<dyn ScriptedLlmProvider>,
) -> std::sync::Arc<crate::ports::ProviderBindingData> {
    let model = provider::ModelInfo {
        provider: provider.provider_name().to_string(),
        model: provider.model_name().to_string(),
        supports_tools: true,
        supports_parallel_tool_calls: true,
        supports_streaming: true,
        supported_reasoning: vec![share::reasoning::ReasoningLevel::Off],
        context_limit: Some(128_000),
        output_limit: Some(8_192),
    };
    std::sync::Arc::new(crate::ports::ProviderBindingData {
        provider: std::sync::Arc::new(ScriptedProviderPortAdapter::new(provider)),
        model,
        max_tokens: 8192,
        requested_reasoning: crate::ports::provider_port::ReasoningLevel::Off,
    })
}
