use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream;
use share::message::Message;
use share::reasoning::ReasoningLevel;
use tokio_util::sync::CancellationToken;

use super::super::client::{LlmClient, ProviderClientSpecData};
use crate::ports::LlmProvider;
use crate::published_language::{
    ProviderContentData, ProviderError, ProviderErrorKind, ProviderResponseChunk,
    ProviderResponseStream, ResponseStopReason,
};

/// v3 拆帧形态的探测成功序列：Content 片段 + 尾帧 Stop（无 usage 上报）。
fn completed() -> Vec<ProviderResponseChunk> {
    vec![
        ProviderResponseChunk::Content(ProviderContentData::Text("OK".to_string())),
        ProviderResponseChunk::Stop(ResponseStopReason::EndTurn),
    ]
}

/// 事件回放 fake：回放固定事件序列并断言探测调用的 scope 语义。
struct EventProvider {
    events: Vec<ProviderResponseChunk>,
    delay: Option<Duration>,
}

#[async_trait]
impl LlmProvider for EventProvider {
    async fn invocation_stream(
        &self,
        resolved: &crate::ports::ResolvedInvocation,
        _system: &str,
        _static_prefix_len: usize,
        messages: &[Message],
        _tools: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<ProviderResponseStream, ProviderError> {
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        // 探测调用语义锁定：单 token + Off 推理 + 单条 user 消息。
        assert_eq!(resolved.max_tokens, 1);
        assert_eq!(resolved.requested_reasoning, ReasoningLevel::Off);
        assert_eq!(messages.len(), 1);
        Ok(Box::pin(stream::iter(self.events.clone())))
    }

    fn model_name(&self) -> &str {
        "probe-model"
    }

    fn provider_name(&self) -> &str {
        "Anthropic"
    }
}

fn probe_client(events: Vec<ProviderResponseChunk>, delay: Option<Duration>) -> Arc<LlmClient> {
    Arc::new(LlmClient::from_provider(Arc::new(EventProvider {
        events,
        delay,
    })))
}

#[tokio::test]
async fn connectivity_probe_returns_latency_on_completed() {
    let client = probe_client(completed(), None);
    let latency = super::run_connectivity_probe(&client, Duration::from_secs(5))
        .await
        .expect("completed event must resolve");
    assert!(!latency.is_zero());
}

#[tokio::test]
async fn connectivity_probe_requires_completed_terminal_event() {
    let client = probe_client(Vec::new(), None);
    let error = super::run_connectivity_probe(&client, Duration::from_secs(5))
        .await
        .expect_err("stream without terminal must fail");
    assert_eq!(error.kind, ProviderErrorKind::Protocol);
}

#[tokio::test]
async fn connectivity_probe_propagates_failed_event_error() {
    let failed = ProviderResponseChunk::Error(ProviderError::fatal(
        ProviderErrorKind::Authentication,
        "upstream rejected",
    ));
    let client = probe_client(vec![failed], None);
    let error = super::run_connectivity_probe(&client, Duration::from_secs(5))
        .await
        .expect_err("failed event must propagate");
    assert_eq!(error.kind, ProviderErrorKind::Authentication);
}

/// 超时语义为秒级粒度（`as_secs().max(1)`，亚秒抬到 1s——沿用组合根既有
/// 语义）；paused clock 下即时验证：上游 120s 慢响应 + 30s 探测超时。
#[tokio::test(start_paused = true)]
async fn connectivity_probe_times_out_and_reports_timeout_kind() {
    let client = probe_client(completed(), Some(Duration::from_secs(120)));
    let error = super::run_connectivity_probe(&client, Duration::from_secs(30))
        .await
        .expect_err("slow upstream must time out");
    assert_eq!(error.kind, ProviderErrorKind::Timeout);
}

fn probe_config() -> ProviderClientSpecData {
    ProviderClientSpecData {
        driver: "anthropic".to_string(),
        source_key: "connect-probe".to_string(),
        api_style: None,
        api_key: "test-api-key".to_string(),
        base_url: Some("https://api.anthropic.com".to_string()),
        model: "probe-model".to_string(),
        max_tokens: 1,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 30,
        user_agent: Some("aemeath/test".to_string()),
    }
}

#[test]
fn wire_probe_client_builds_from_valid_config() {
    let client = super::wire_probe_client(probe_config()).expect("valid config must build");
    assert_eq!(client.model_name(), "probe-model");
}

#[test]
fn wire_probe_client_folds_config_failure_with_fixed_message() {
    let mut config = probe_config();
    config.driver = "not-a-real-driver".to_string();
    let error = match super::wire_probe_client(config) {
        Err(error) => error,
        Ok(_) => panic!("unknown driver must fail"),
    };
    assert_eq!(error.kind, ProviderErrorKind::Configuration);
    assert_eq!(error.safe_message, "连接测试配置无效");
}
