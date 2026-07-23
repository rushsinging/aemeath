use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use provider::composition::{InvocationScope, LlmClient, LlmConfigOptions, LlmError, SystemBlock};
use provider::{
    CancellationSignal, InvocationRequest, InvocationStream, ModelCapability, ModelId,
    ProviderError, ProviderErrorKind, ReasoningCapability, ReasoningLevel, ReasoningMappingKind,
};
use runtime::{
    ProviderBinding, ProviderBuildSpec, ProviderFactory as ProviderFactoryTrait, ProviderPort,
};

// ─── New adapter: ProviderPort via LlmClient ────────────────

/// Composition-owned adapter: wraps the Provider construction handle and a set of
/// model capabilities to expose the `runtime::ports::ProviderPort` contract.
///
/// The adapter lives in the Composition crate because the dependency
/// direction is `runtime → provider`; Composition depends on both.
pub struct ProviderAdapter {
    client: Arc<LlmClient>,
    capabilities: HashMap<ModelId, ModelCapability>,
}

impl ProviderAdapter {
    /// Create a new adapter over an opaque LLM client and its known capabilities.
    pub fn new(client: Arc<LlmClient>, capabilities: HashMap<ModelId, ModelCapability>) -> Self {
        Self {
            client,
            capabilities,
        }
    }
}

/// Production factory: accepts an opaque Provider construction handle and a map of
/// model capabilities, returns `Arc<dyn ProviderPort>` backed by a
/// Composition-owned adapter.
pub fn provider_port(
    client: Arc<LlmClient>,
    capabilities: HashMap<ModelId, ModelCapability>,
) -> Arc<dyn ProviderPort> {
    Arc::new(ProviderAdapter::new(client, capabilities))
}

#[async_trait]
impl ProviderPort for ProviderAdapter {
    fn capabilities(&self, model: &ModelId) -> Result<ModelCapability, ProviderError> {
        self.capabilities.get(model).cloned().ok_or_else(|| {
            ProviderError::fatal(
                ProviderErrorKind::ModelUnavailable,
                format!("unknown model: {model}"),
            )
        })
    }

    async fn invoke(
        &self,
        request: InvocationRequest,
        cancellation: &dyn CancellationSignal,
    ) -> Result<InvocationStream, ProviderError> {
        // Fast path: signal already fired.
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled());
        }

        // (4) Reject unknown models and clamp requested reasoning to the
        // declared capability. The adapter owns the clamp; the underlying
        // provider is never asked to reason above what the model supports.
        let capability = self.capabilities(&request.model)?;
        let requested_reasoning = request.options.reasoning;
        let effective_reasoning = capability.reasoning.resolve(requested_reasoning);

        // (3) Scope model uses the provider-neutral model name
        // (`request.model.model`), NOT the composite "provider/model" string.
        let scope = InvocationScope::new(
            request.model.model.clone(),
            request.options.max_output_tokens,
            requested_reasoning,
            effective_reasoning,
        )
        .map_err(|e| {
            ProviderError::fatal(
                ProviderErrorKind::Configuration,
                format!("invalid scope: {e}"),
            )
        })?;

        // (2) Convert provider-neutral system blocks into the legacy
        // `provider::SystemBlock` — `Cacheable` → ephemeral cached block,
        // `Text` → dynamic (uncached) block.
        let system_blocks: Vec<SystemBlock> = request
            .system
            .iter()
            .map(|block| match block {
                provider::RequestSystemBlock::Text(text) => SystemBlock::dynamic(text.clone()),
                provider::RequestSystemBlock::Cacheable(text) => SystemBlock::cached(text.clone()),
            })
            .collect();

        // (2) Convert each ModelToolSchema into a complete wire JSON object
        // {name, description, input_schema} rather than passing the bare
        // input_schema. The JSON is built inside the provider crate
        // (`ModelToolSchema::to_tool_definition`) so Composition does not need
        // a direct serde_json dependency.
        let tool_schemas: Vec<_> = request
            .tools
            .iter()
            .map(|tool| tool.to_tool_definition())
            .collect();

        // The request carries the Runtime-owned token that remains alive for the
        // producer lifetime after this method returns.
        let cancel_token = request.cancellation.clone();
        let establishment = self.client.invocation_stream(
            &scope,
            &system_blocks,
            &request.messages,
            &tool_schemas,
            &cancel_token,
        );
        tokio::pin!(establishment);

        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                cancel_token.cancel();
                return Err(ProviderError::cancelled());
            }
            result = &mut establishment => result,
        };

        result
    }
}

// ─── ProviderFactory implementation ─────────────────────

/// Default `ProviderFactory` implementation: builds a `ProviderBinding` from a
/// `ProviderBuildSpec` through the Provider-owned Composition construction API,
/// building a `ModelCapability` from the client's max reasoning level and spec
/// limits, and wrapping the client in the existing `ProviderAdapter`.
pub struct DefaultProviderFactory;

/// Convenience constructor: returns a boxed `ProviderFactory`.
pub fn provider_factory() -> Arc<dyn ProviderFactoryTrait> {
    Arc::new(DefaultProviderFactory)
}

impl ProviderFactoryTrait for DefaultProviderFactory {
    fn build(&self, spec: ProviderBuildSpec) -> Result<ProviderBinding, ProviderError> {
        let config = LlmConfigOptions {
            driver: spec.driver.clone(),
            source_key: spec.source_key.clone(),
            api_style: spec.api_style.clone(),
            api_key: spec.api_key.clone(),
            base_url: spec.base_url.clone(),
            model: spec.model.model.clone(),
            max_tokens: spec.max_tokens,
            reasoning: spec.requested_reasoning != ReasoningLevel::Off,
            reasoning_config: None,
            timeout_secs: spec.timeout.as_secs(),
            user_agent: Some(spec.user_agent),
        };

        let client = LlmClient::from_config(config).map_err(|err| {
            let kind = match &err {
                LlmError::Cancelled => ProviderErrorKind::Cancelled,
                LlmError::RateLimited => ProviderErrorKind::RateLimited,
                LlmError::ContextTooLong => ProviderErrorKind::ContextTooLong,
                LlmError::Network(_) => ProviderErrorKind::Network,
                LlmError::Api { .. } => ProviderErrorKind::UpstreamUnavailable,
                LlmError::Stream(_) => ProviderErrorKind::Protocol,
                LlmError::StreamInterrupted(_) | LlmError::StreamTruncated { .. } => {
                    ProviderErrorKind::StreamTruncated
                }
                LlmError::Config(_) => ProviderErrorKind::Configuration,
            };
            ProviderError::fatal(kind, err.to_string())
        })?;

        let client = Arc::new(
            client
                .with_default_reasoning(spec.requested_reasoning)
                .map_err(|error| {
                    ProviderError::fatal(ProviderErrorKind::Configuration, error.to_string())
                })?,
        );

        // Build a ReasoningCapability whose supported levels are every level
        // from Off up to the client's reported max reasoning level (inclusive).
        let max_reasoning = client.max_reasoning_level();
        let reasoning_cap = reasoning_capability_from_max(max_reasoning);

        let requested_reasoning = client.default_scope().requested_reasoning();

        let capability = ModelCapability {
            model: spec.model.clone(),
            supports_tools: true,
            supports_parallel_tool_calls: true,
            supports_streaming: true,
            reasoning: reasoning_cap,
            context_limit: spec.context_window,
            output_limit: Some(spec.max_tokens as usize),
        };

        let capabilities = HashMap::from([(spec.model.clone(), capability)]);
        let port = provider_port(client, capabilities);

        Ok(ProviderBinding {
            provider: port,
            model: spec.model,
            max_tokens: spec.max_tokens,
            requested_reasoning,
            context_window: spec.context_window,
        })
    }
}

/// Build a `ReasoningCapability` that supports every level from `Off` up to
/// and including `max`.
fn reasoning_capability_from_max(max: ReasoningLevel) -> ReasoningCapability {
    let all_levels = [
        ReasoningLevel::Off,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
        ReasoningLevel::Xhigh,
        ReasoningLevel::Max,
    ];
    let supported: Vec<_> = all_levels.into_iter().filter(|l| *l <= max).collect();
    ReasoningCapability::new(supported, ReasoningMappingKind::Effort)
        .unwrap_or_else(|_| ReasoningCapability::none())
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
