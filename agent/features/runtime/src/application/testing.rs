#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

fn test_workspaces(
) -> &'static Mutex<HashMap<String, crate::adapters::tool_runtime::RuntimeWorkspaceAccess>> {
    static WORKSPACES: OnceLock<
        Mutex<HashMap<String, crate::adapters::tool_runtime::RuntimeWorkspaceAccess>>,
    > = OnceLock::new();
    WORKSPACES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn test_tool_execution_context(
    root: std::path::PathBuf,
    cancel: tokio_util::sync::CancellationToken,
) -> tools::ToolExecutionContext {
    let views = project::wire_production_workspace(root.clone())
        .expect("workspace initialization")
        .into_views();
    let workspace = crate::adapters::tool_runtime::RuntimeWorkspaceAccess::new(views.clone());
    test_workspaces().lock().expect("test workspaces").insert(
        views.read().workspace_id().as_str().to_string(),
        workspace.clone(),
    );
    tools::ToolExecutionContext::new(
        tools::ExecutionScope::builder("test-run", views.read().workspace_id(), root).build(),
        tools::ToolExecutionPorts::new(
            crate::adapters::tool_runtime::cancellation(cancel.clone()),
            workspace.read_access(),
            Arc::new(tools::MutexReadSet(Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )))),
            Arc::new(tools::FixedPlanMode(None)),
            Arc::new(memory::NoOpMemory),
            Arc::new(tools::FixedGuidance {
                language: "en".into(),
            }),
        ),
    )
}

pub(crate) fn runtime_workspace(
    ctx: &tools::ToolExecutionContext,
) -> crate::adapters::tool_runtime::RuntimeWorkspaceAccess {
    test_workspaces()
        .lock()
        .expect("test workspaces")
        .get(ctx.scope().workspace_id().as_str())
        .expect("context workspace backing")
        .clone()
}

pub(crate) fn workspace_persist(
    ctx: &tools::ToolExecutionContext,
) -> Arc<dyn project::WorkspacePersist> {
    runtime_workspace(ctx).persist()
}

use async_trait::async_trait;
use futures::stream;
use provider::{
    InvocationDelta, InvocationEvent, InvocationStream, ProviderCompletion, ProviderContentBlock,
    ProviderStopReason, RawUsageSnapshot, ReasoningLevel,
};

pub(crate) fn test_tool_result_materializer(
) -> Arc<crate::application::tool_result_materialization::ToolResultMaterializer> {
    struct TestBlobPort;

    #[async_trait]
    impl crate::ports::ToolResultBlobPort for TestBlobPort {
        async fn write_once(
            &self,
            session_id: &str,
            tool_use_id: &str,
            _bytes: &[u8],
        ) -> Result<crate::ports::ToolResultBlobRef, crate::ports::ToolResultBlobError> {
            Ok(crate::ports::ToolResultBlobRef::new(format!(
                "tool-result://{session_id}/{tool_use_id}"
            )))
        }
    }

    Arc::new(
        crate::application::tool_result_materialization::ToolResultMaterializer::new(
            Arc::new(TestBlobPort),
            crate::application::tool_result_materialization::ToolResultMaterializationPolicy::new(
                50_000, 2_000, 500,
            ),
        ),
    )
}

pub(crate) fn text_completion_stream(
    text: impl Into<String>,
    input_tokens: u32,
    output_tokens: u32,
) -> InvocationStream {
    let text = text.into();
    Box::pin(stream::iter([
        InvocationEvent::Delta(InvocationDelta::Text(text.clone())),
        InvocationEvent::Completed(ProviderCompletion {
            output: vec![ProviderContentBlock::Text(text)],
            stop_reason: ProviderStopReason::EndTurn,
            usage: Some(RawUsageSnapshot {
                input_tokens: Some(input_tokens),
                output_tokens: Some(output_tokens),
                ..RawUsageSnapshot::default()
            }),
            effective_reasoning: ReasoningLevel::Off,
        }),
    ]))
}

// ─── Test ProviderPort helpers (#907) ────────────────────────────

use std::collections::VecDeque;

/// Per-call custom invocation hook for `TestProviderPort` (#907 loop test migration).
///
/// Receives `(call_index, request, cancellation)` and must return the future of
/// the resulting invocation stream (or error). When set via `with_invocation_fn`,
/// it **fully overrides** the default `error → blocking → cancel → responses-queue`
/// dispatch. Tests use this to keep `Sequence`/`recording`/`error`/`cancel` behavior
/// without writing bespoke provider port impls.
///
/// Uses `for<'a>` HRTB so closures can capture the borrowed `&InvocationRequest`
/// / `&dyn CancellationSignal` into their returned `Future + 'a`.
pub(crate) type TestInvocationFn = Arc<
    dyn for<'a> Fn(
            usize,
            &'a crate::ports::provider_port::InvocationRequest,
            &'a dyn crate::ports::provider_port::CancellationSignal,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            crate::ports::provider_port::InvocationStream,
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
    pub model: provider::ModelId,
    pub blocking: bool,
    pub seen: Option<Arc<Mutex<Vec<::logging::LogContext>>>>,
    pub calls: Arc<Mutex<usize>>,
    /// Optional per-call hook overriding default dispatch (see [`TestInvocationFn`]).
    pub invocation_fn: Option<TestInvocationFn>,
}

impl TestProviderPort {
    pub fn new(responses: Vec<&str>, model: provider::ModelId) -> Self {
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

    pub fn with_error(mut self, error: crate::ports::provider_port::ProviderError) -> Self {
        self.error = Some(error);
        self
    }
    pub fn with_blocking(mut self) -> Self {
        self.blocking = true;
        self
    }
    pub fn with_seen(mut self, seen: Arc<Mutex<Vec<::logging::LogContext>>>) -> Self {
        self.seen = Some(seen);
        self
    }
    pub fn with_calls(mut self, calls: Arc<Mutex<usize>>) -> Self {
        self.calls = calls;
        self
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
        model: &provider::ModelId,
    ) -> Result<
        crate::ports::provider_port::ModelCapability,
        crate::ports::provider_port::ProviderError,
    > {
        use crate::ports::provider_port::{
            ModelCapability, ProviderError, ProviderErrorKind, ReasoningCapability,
        };
        if model == &self.model {
            Ok(ModelCapability {
                model: model.clone(),
                supports_tools: true,
                supports_parallel_tool_calls: true,
                supports_streaming: true,
                reasoning: ReasoningCapability::none(),
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
        request: crate::ports::provider_port::InvocationRequest,
        cancellation: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::InvocationStream,
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

/// Default `ModelId` used by `test_binding*` helpers.
pub(crate) fn test_model_id() -> provider::ModelId {
    provider::ModelId {
        provider: "test".to_string(),
        model: "test-model".to_string(),
    }
}

/// Build a `ProviderBinding` whose provider records the last user message of each
/// invocation and returns `"response to {last_user}"`.
///
/// Returns the binding and a handle to the recorded message list. Use this to
/// replace bespoke `RecordingProvider` impls in loop tests.
pub(crate) fn recording_test_binding(
) -> (Arc<crate::ports::ProviderBinding>, Arc<Mutex<Vec<String>>>) {
    let recorded: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded_for_hook = recorded.clone();
    let port = TestProviderPort::new(Vec::new(), test_model_id()).with_invocation_fn(Arc::new(
        move |_call_index, request, _cancel| {
            let recorded = recorded_for_hook.clone();
            Box::pin(async move {
                let last_user = request
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == share::message::Role::User)
                    .map(|m| m.text_content())
                    .unwrap_or_default();
                recorded.lock().unwrap().push(last_user.clone());
                Ok(text_completion_stream(
                    format!("response to {last_user}"),
                    1,
                    1,
                ))
            })
        },
    ));
    (test_binding_from_port(port), recorded)
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
/// existing `provider::test_harness::LlmProvider` scripted fake.
///
/// Used only by `runtime` lib tests as a minimal bridge so the legacy scripted
/// fakes (e.g. `SequenceProvider`, `RecordingProvider`, `CountingProvider`,
/// `ErrorProvider`) can be wrapped in a `ProviderBinding` without rewriting
/// every test to the new `ProviderPort` trait.
struct LlmProviderPortAdapter {
    provider: std::sync::Arc<dyn provider::test_harness::LlmProvider>,
    model: provider::ModelId,
}

impl LlmProviderPortAdapter {
    fn new(provider: std::sync::Arc<dyn provider::test_harness::LlmProvider>) -> Self {
        let model = provider::ModelId {
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
        model: &provider::ModelId,
    ) -> Result<
        crate::ports::provider_port::ModelCapability,
        crate::ports::provider_port::ProviderError,
    > {
        use crate::ports::provider_port::{
            ModelCapability, ProviderError, ProviderErrorKind, ReasoningCapability,
        };
        if model == &self.model {
            Ok(ModelCapability {
                model: model.clone(),
                supports_tools: true,
                supports_parallel_tool_calls: true,
                supports_streaming: true,
                reasoning: ReasoningCapability::none(),
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
        request: crate::ports::provider_port::InvocationRequest,
        cancellation: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::InvocationStream,
        crate::ports::provider_port::ProviderError,
    > {
        // Convert InvocationRequest into the legacy LlmProvider argument list.
        let system_blocks: Vec<provider::test_harness::SystemBlock> = request
            .system
            .iter()
            .map(|block| provider::test_harness::SystemBlock::dynamic(block.text().to_string()))
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
        let scope = provider::test_harness::InvocationScope::new(
            self.model.model.clone(),
            request.options.max_output_tokens.max(1),
            provider::ReasoningLevel::Off,
            provider::ReasoningLevel::Off,
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

/// Wrap an existing `provider::test_harness::LlmProvider` scripted fake into a
/// `ProviderBinding` so legacy loop/agent tests can plug their fakes directly
/// into the `binding` field on `ChatLoopContext` / `RuntimeResources` without
/// rewriting the fake bodies.
///
/// The binding's `model`/`max_tokens`/`context_window` mirror the values used by
/// the script fakes' default `LlmClient::from_provider(...)` construction.
pub(crate) fn binding_from_llm_provider(
    provider: std::sync::Arc<dyn provider::test_harness::LlmProvider>,
) -> std::sync::Arc<crate::ports::ProviderBinding> {
    let model = provider::ModelId {
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

// ─── Test ModelsConfig helpers (#907) ─────────────────────────

/// Build a `ModelsConfig` with one provider/model pair so the runner's
/// `find_model` lookup succeeds for `<provider_key>/<model_id>`.
///
/// Tests use this when exercising `model_spec = Some("provider/model")`
/// resolution paths without spinning up real model configs.
pub(crate) fn models_config_with_model(
    provider_key: &str,
    model_id: &str,
) -> share::config::ModelsConfig {
    use share::config::models::{ModelEntryConfig, ProviderModelsConfig};
    let mut providers = std::collections::HashMap::new();
    providers.insert(
        provider_key.to_string(),
        ProviderModelsConfig {
            driver: "openai".to_string(),
            api_key: "test-key".to_string(),
            models: vec![ModelEntryConfig {
                id: model_id.to_string(),
                name: model_id.to_string(),
                context_window: 128_000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    share::config::ModelsConfig {
        providers,
        ..Default::default()
    }
}
