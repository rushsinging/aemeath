use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use provider::composition::{LlmClient, LlmConfigOptionsData};
use provider::{
    InvocationRequestData, InvocationStreamData, ModelCapabilityData, ModelIdData, ProviderError,
    ProviderErrorKind,
};

use runtime::{
    ProviderBindingData, ProviderBuildSpecData, ProviderFactory as ProviderFactoryTrait,
    ProviderPort,
};
use share::reasoning::ReasoningLevel;

// ─── New adapter: ProviderPort via LlmClient ────────────────

/// Composition-owned adapter: wraps the Provider construction handle and a set of
/// model capabilities to expose the `runtime::ports::ProviderPort` contract.
///
/// The adapter lives in the Composition crate because the dependency
/// direction is `runtime → provider`; Composition depends on both.
pub struct ProviderAdapter {
    client: Arc<LlmClient>,
    capabilities: HashMap<ModelIdData, ModelCapabilityData>,
}

impl ProviderAdapter {
    /// Create a new adapter over an opaque LLM client and its known capabilities.
    pub fn new(
        client: Arc<LlmClient>,
        capabilities: HashMap<ModelIdData, ModelCapabilityData>,
    ) -> Self {
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
    capabilities: HashMap<ModelIdData, ModelCapabilityData>,
) -> Arc<dyn ProviderPort> {
    Arc::new(ProviderAdapter::new(client, capabilities))
}

#[async_trait]
impl ProviderPort for ProviderAdapter {
    fn capabilities(&self, model: &ModelIdData) -> Result<ModelCapabilityData, ProviderError> {
        self.capabilities.get(model).cloned().ok_or_else(|| {
            ProviderError::fatal(
                ProviderErrorKind::ModelUnavailable,
                format!("unknown model: {model}"),
            )
        })
    }

    async fn invoke(
        &self,
        request: InvocationRequestData,
        cancellation: &dyn runtime::CancellationSignal,
    ) -> Result<InvocationStreamData, ProviderError> {
        // fast path：调用方信号已触发。
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        let capability = self.capabilities(&request.model)?;
        self.client.invoke(&capability, &request).await
    }
}

// ─── ProviderFactory implementation ─────────────────────

/// Default `ProviderFactory` implementation: builds a `ProviderBindingData` from a
/// `ProviderBuildSpecData` through the Provider-owned Composition construction API,
/// building a `ModelCapabilityData` from the client's max reasoning level and spec
/// limits, and wrapping the client in the existing `ProviderAdapter`.
///
/// 持有进程级 `TransportPool`：同 transport key（driver/endpoint/认证域/
/// user-agent/timeout）的多次 build 复用同一不可变 transport；model /
/// max_tokens / reasoning 属于 invocation 配置，不参与复用判定。
pub struct DefaultProviderFactory {
    pool: Arc<provider::composition::TransportPool>,
}

impl DefaultProviderFactory {
    pub fn new() -> Self {
        Self {
            pool: Arc::new(provider::composition::TransportPool::new()),
        }
    }

    /// 共享的 transport pool；诊断与契约测试用。
    pub fn shared_pool(&self) -> &Arc<provider::composition::TransportPool> {
        &self.pool
    }
}

impl Default for DefaultProviderFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience constructor: returns a pooled provider factory.
///
/// 调用方应装配一次并复用（pool 生命周期与 factory 实例一致）；重复调用
/// 会产生独立 pool，失去跨调用复用。
pub fn provider_factory() -> Arc<DefaultProviderFactory> {
    Arc::new(DefaultProviderFactory::new())
}

impl ProviderFactoryTrait for DefaultProviderFactory {
    fn build(&self, spec: ProviderBuildSpecData) -> Result<ProviderBindingData, ProviderError> {
        let config = LlmConfigOptionsData {
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

        let assembly = provider::composition::wire_provider_assembly(
            config,
            spec.model.clone(),
            self.pool.as_ref(),
            spec.requested_reasoning,
            spec.context_window,
            spec.max_tokens,
        )?;

        let capabilities = HashMap::from([(spec.model.clone(), assembly.capability)]);
        let port = provider_port(assembly.client, capabilities);

        Ok(ProviderBindingData {
            provider: port,
            model: spec.model,
            max_tokens: spec.max_tokens,
            requested_reasoning: assembly.requested_reasoning,
            context_window: spec.context_window,
        })
    }
}

use config::ports::{
    ProviderProbeError, ProviderProbeErrorKind, ProviderProbePort, ProviderProbeRequest,
    ProviderProbeResult,
};

/// Connect 向导探测桥接：config 端口请求 → provider 探测内核。
///
/// 探测调用语义（单 token/Off/事件消费/超时取消）归 provider
/// `run_connectivity_probe`；本层只做请求翻译与错误文案映射。
pub struct ProviderProbeAdapter;

impl ProviderProbeAdapter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl ProviderProbePort for ProviderProbeAdapter {
    async fn probe(
        &self,
        request: ProviderProbeRequest,
    ) -> Result<ProviderProbeResult, ProviderProbeError> {
        let config = probe_config_from_request(&request);
        let client = provider::composition::wire_probe_client(config).map_err(map_probe_error)?;
        provider::composition::run_connectivity_probe(&client, request.timeout)
            .await
            .map(|latency| ProviderProbeResult { latency })
            .map_err(map_probe_error)
    }
}

/// Connect 探测请求 → provider 构造配置的纯翻译（无 IO）。
fn probe_config_from_request(request: &ProviderProbeRequest) -> LlmConfigOptionsData {
    LlmConfigOptionsData {
        driver: request.driver.as_str().to_string(),
        source_key: "connect-probe".to_string(),
        api_style: request.api_style.clone(),
        api_key: request.credential.clone().unwrap_or_default(),
        base_url: Some(request.base_url.clone()),
        model: request.model_id.clone(),
        max_tokens: 1,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: request.timeout.as_secs().max(1),
        user_agent: Some(request.final_user_agent.clone()),
    }
}

fn map_probe_error(error: ProviderError) -> ProviderProbeError {
    let kind = match error.kind {
        ProviderErrorKind::Cancelled => ProviderProbeErrorKind::Cancelled,
        ProviderErrorKind::Timeout => ProviderProbeErrorKind::Timeout,
        ProviderErrorKind::Authentication | ProviderErrorKind::PermissionDenied => {
            ProviderProbeErrorKind::Authentication
        }
        ProviderErrorKind::ModelUnavailable
        | ProviderErrorKind::ContextTooLong
        | ProviderErrorKind::InvalidRequest => ProviderProbeErrorKind::Model,
        ProviderErrorKind::Protocol | ProviderErrorKind::StreamTruncated => {
            ProviderProbeErrorKind::Protocol
        }
        ProviderErrorKind::Network | ProviderErrorKind::UpstreamUnavailable => {
            ProviderProbeErrorKind::Endpoint
        }
        ProviderErrorKind::RateLimited | ProviderErrorKind::Configuration => {
            ProviderProbeErrorKind::Internal
        }
    };
    let message = match kind {
        ProviderProbeErrorKind::Cancelled => "连接测试已取消",
        ProviderProbeErrorKind::Timeout => "连接测试超时",
        ProviderProbeErrorKind::Authentication => "认证失败",
        ProviderProbeErrorKind::Endpoint => "服务地址不可用",
        ProviderProbeErrorKind::Model => "模型不可用",
        ProviderProbeErrorKind::Protocol => "服务响应协议无效",
        ProviderProbeErrorKind::Internal => "连接测试失败",
    };
    ProviderProbeError {
        kind,
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "provider_probe_tests.rs"]
mod probe_tests;
