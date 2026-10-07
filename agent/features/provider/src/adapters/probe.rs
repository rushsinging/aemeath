//! Connect 向导探测内核：单 token Off 探测调用的执行语义。

use std::sync::Arc;
use std::time::Duration;

use share::message::Message;
use share::reasoning::ReasoningLevel;

use super::client::{LlmClient, LlmConfigOptionsData};

/// 构造探测专用客户端（独立 transport，不走 pool）。
///
/// 探测是 connect 向导的一次性调用，与主链路客户端生命周期无关；
/// 构造失败统一折叠为 Configuration（"连接测试配置无效"）。
pub fn wire_probe_client(
    options: LlmConfigOptionsData,
) -> Result<Arc<LlmClient>, crate::ProviderError> {
    LlmClient::from_config(options).map(Arc::new).map_err(|_| {
        crate::ProviderError::fatal(crate::ProviderErrorKind::Configuration, "连接测试配置无效")
    })
}

/// Connect 向导探测唯一入口：构造探测客户端并执行一次连通性探测。
///
/// 组合根 NEVER 自行拼装探测客户端（#1861 C14 收敛：构造与执行合一）。
pub async fn probe_connectivity(
    options: LlmConfigOptionsData,
    timeout: Duration,
) -> Result<Duration, crate::ProviderError> {
    let client = wire_probe_client(options)?;
    run_connectivity_probe(&client, timeout).await
}

/// 执行一次连通性探测：单 token、Off 推理、"Reply with OK." 单条消息，
/// 经非流式 [`LlmClient::invoke`] 聚合判定 `response.ok`；`timeout` 到期
/// 即取消并返回 Timeout。
///
/// 返回测量延迟；失败透传 `response.error`（无错误载荷时构造 Protocol）。
/// 探测调用语义（max_tokens=1 / Off / 单消息）属于 provider 知识，
/// 组合根 NEVER 自行拼装。
pub async fn run_connectivity_probe(
    client: &LlmClient,
    timeout: Duration,
) -> Result<Duration, crate::ProviderError> {
    let started = std::time::Instant::now();
    let timeout_secs = timeout.as_secs().max(1);
    let model = crate::ModelIdData {
        provider: client.provider_name().to_string(),
        model: client.model_name().to_string(),
    };
    let capability = crate::ModelCapabilityData {
        model: model.clone(),
        supports_tools: false,
        supports_parallel_tool_calls: false,
        supports_streaming: true,
        reasoning: crate::ReasoningCapabilityData::none(),
        context_limit: None,
        output_limit: None,
    };
    let mut request = crate::InvocationRequestData::new(
        model,
        [Message::user("Reply with OK.")],
        1,
        ReasoningLevel::Off,
    );
    let cancellation = tokio_util::sync::CancellationToken::new();
    request.cancellation = cancellation.clone();
    let operation = async {
        let response = client.invoke(&capability, &request).await?;
        if response.ok {
            return Ok(started.elapsed());
        }
        Err(response.error.unwrap_or_else(|| {
            crate::ProviderError::fatal(
                crate::ProviderErrorKind::Protocol,
                "服务响应未包含完成事件",
            )
        }))
    };
    match tokio::time::timeout(Duration::from_secs(timeout_secs), operation).await {
        Ok(result) => result,
        Err(_) => {
            cancellation.cancel();
            Err(crate::ProviderError::fatal(
                crate::ProviderErrorKind::Timeout,
                "连接测试超时",
            ))
        }
    }
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
