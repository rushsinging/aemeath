use std::sync::Arc;

use provider::composition::ProviderClientSpecData;
use provider::{ModelInfo, ProviderError, ProviderErrorKind};

use runtime::{
    ProviderBindingData, ProviderBuildSpecData, ProviderFactory as ProviderFactoryTrait,
};
use share::reasoning::ReasoningLevel;

// ─── ProviderFactory implementation ─────────────────────

/// Default `ProviderFactory` implementation: builds a `ProviderBindingData` from a
/// `ProviderBuildSpecData` through the Provider-owned Composition construction API,
/// constructing the binding's single `ModelInfo` (身份/支持面/调用限制来自 spec，
/// reasoning 阶梯由装配时 client 推导——provider 读侧权威)，and wrapping the
/// client in the existing `ProviderAdapter`.
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
        let config = ProviderClientSpecData {
            driver: spec.driver.clone(),
            source_key: spec.source_key.clone(),
            api_style: spec.api_style.clone(),
            api_key: spec.api_key.clone(),
            base_url: spec.base_url.clone(),
            model: spec.model.clone(),
            max_tokens: spec.max_tokens,
            reasoning: spec.requested_reasoning != ReasoningLevel::Off,
            reasoning_config: None,
            timeout_secs: spec.timeout.as_secs(),
            user_agent: Some(spec.user_agent),
        };

        // 组合根从 config/spec 投影构造 ModelInfo；supported_reasoning 阶梯
        // 与 server_effort_map 均由装配（client 能力 + driver 归并）覆盖——
        // 此处填占位。
        let model = ModelInfo {
            provider: spec.source_key.clone(),
            model: spec.model.clone(),
            supports_tools: true,
            supports_parallel_tool_calls: true,
            supports_streaming: true,
            supported_reasoning: vec![ReasoningLevel::Off],
            server_effort_map: Vec::new(),
            context_limit: spec.context_window,
            output_limit: Some(spec.max_tokens as usize),
        };

        let (client, model) =
            provider::composition::wire_provider_client(config, model, self.pool.as_ref())?;

        // #2146 单端口：同一 `Arc<LlmClient>` 既是会话流式通道（invoke_stream）
        // 也是 compact 文本通道（invoke_text 默认方法）——能力与 model 恒同源
        // wire，跨 driver 的 compact_model 配置天然 transport 匹配。
        let provider: Arc<dyn provider::LlmInvokePort> = client;

        Ok(ProviderBindingData {
            provider,
            model,
            max_tokens: spec.max_tokens,
            // #1861 v4：binding 档位从 spec 直取（原 wiring.requested_reasoning
            // 与输入重复，随装配句柄包消除）。
            requested_reasoning: spec.requested_reasoning,
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

#[async_trait::async_trait]
impl ProviderProbePort for ProviderProbeAdapter {
    async fn probe(
        &self,
        request: ProviderProbeRequest,
    ) -> Result<ProviderProbeResult, ProviderProbeError> {
        let config = probe_config_from_request(&request);
        provider::composition::wire_test_provider_client(config, request.timeout)
            .await
            .map(|latency| ProviderProbeResult { latency })
            .map_err(map_probe_error)
    }
}

/// Connect 探测请求 → provider 构造配置的纯翻译（无 IO）。
fn probe_config_from_request(request: &ProviderProbeRequest) -> ProviderClientSpecData {
    ProviderClientSpecData {
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
