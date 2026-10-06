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

/// 执行一次连通性探测：单 token、Off 推理、"Reply with OK." 单条消息，
/// 消费事件流直到 Completed 终态；`timeout` 到期即取消并返回 Timeout。
///
/// 返回测量延迟；流中 Failed 事件透传其错误；流结束未见 Completed
/// 返回 Protocol。探测调用语义（max_tokens=1 / Off / 单消息）属于
/// provider 知识，组合根 NEVER 自行拼装。
pub async fn run_connectivity_probe(
    client: &LlmClient,
    timeout: Duration,
) -> Result<Duration, crate::ProviderError> {
    let started = std::time::Instant::now();
    let timeout_secs = timeout.as_secs().max(1);
    let scope = client
        .invocation_scope(client.model_name(), Some(1), ReasoningLevel::Off)
        .map_err(crate::ProviderError::from)?;
    let messages = [Message::user("Reply with OK.")];
    let cancellation = tokio_util::sync::CancellationToken::new();
    let operation = async {
        let mut stream = client
            .invocation_stream(&scope, &[], &messages, &[], &cancellation)
            .await?;
        use futures_util::StreamExt;
        while let Some(event) = stream.next().await {
            match event {
                crate::published_language::InvocationEventData::Completed(_) => {
                    return Ok(started.elapsed());
                }
                crate::published_language::InvocationEventData::Failed(error) => return Err(error),
                crate::published_language::InvocationEventData::Delta(_) => {}
            }
        }
        Err(crate::ProviderError::fatal(
            crate::ProviderErrorKind::Protocol,
            "服务响应未包含完成事件",
        ))
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
