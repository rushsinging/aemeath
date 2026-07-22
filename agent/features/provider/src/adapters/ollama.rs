//! Ollama provider implementation — 主模块
//! 本地 Ollama 推理服务优化：更长超时、可选认证、无 stream_options、空响应检测。

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
};
use crate::domain::invoke::{InvocationScope, SystemBlock};
use crate::ports::LlmProvider;

mod conversion;
pub(crate) mod stream;

use conversion::OllamaProviderConversion;

pub struct OllamaProvider {
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) user_agent: String,
    pub(crate) http: reqwest::Client,
    pub(crate) timeout_secs: u64,
}

/// Stream idle timeout（单一真相源：`business::OLLAMA_STREAM_IDLE_TIMEOUT_SECS`）
pub(crate) const STREAM_IDLE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(crate::OLLAMA_STREAM_IDLE_TIMEOUT_SECS);

impl OllamaProvider {
    /// `max_tokens` / `reasoning` 不再作为可变运行时状态保留：每次调用的实际
    /// max_tokens / 推理档位由调用方传入的 `InvocationScope` 决定（不可变、
    /// 一次调用一份快照）。这两个构造参数仅为保持调用方签名兼容而保留，
    /// 当前未参与任何 immutable default 的派生，故有意不使用。
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        _max_tokens: u32,
        _reasoning: bool,
        timeout_secs: u64,
    ) -> Self {
        Self {
            base_url: {
                let url = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());
                url.trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .to_string()
            },
            model: model.unwrap_or_else(|| "llama3.2".to_string()),
            api_key,
            user_agent: format!("aemeath/{}", share::version()),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(crate::CONNECT_TIMEOUT_SECS))
                .build()
                .expect("failed to create HTTP client"),
            timeout_secs,
        }
    }

    #[allow(dead_code)]
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self.http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(crate::CONNECT_TIMEOUT_SECS))
            .build()
            .expect("failed to create HTTP client with custom timeout");
        self
    }

    fn build_headers(&self) -> Result<HeaderMap, crate::LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Ollama doesn't require auth, but send it if provided (for proxy setups)
        if !self.api_key.is_empty() && self.api_key != "ollama" {
            headers.insert(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .map_err(|e| crate::LlmError::Config(e.to_string()))?,
            );
        }
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|e| crate::LlmError::Config(e.to_string()))?,
        );
        Ok(headers)
    }
}

fn provider_error_from_llm(error: crate::LlmError) -> crate::ProviderError {
    let kind = match error {
        crate::LlmError::Cancelled => crate::ProviderErrorKind::Cancelled,
        crate::LlmError::RateLimited => crate::ProviderErrorKind::RateLimited,
        crate::LlmError::ContextTooLong => crate::ProviderErrorKind::ContextTooLong,
        crate::LlmError::Network(_) => crate::ProviderErrorKind::Network,
        crate::LlmError::Api { .. } => crate::ProviderErrorKind::UpstreamUnavailable,
        crate::LlmError::StreamInterrupted(_) | crate::LlmError::StreamTruncated { .. } => {
            crate::ProviderErrorKind::StreamTruncated
        }
        crate::LlmError::Stream(_) => crate::ProviderErrorKind::Protocol,
        crate::LlmError::Config(_) => crate::ProviderErrorKind::Configuration,
    };
    crate::ProviderError::fatal(kind, error.to_string())
}

fn provider_error_from_attempt(failure: HttpAttemptFailure) -> crate::ProviderError {
    failure.into_provider_error()
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn invocation_stream(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStream, crate::ProviderError> {
        if cancel.is_cancelled() {
            return Err(crate::ProviderError::cancelled());
        }
        let request_body = self
            .build_request_body(scope, system, messages, tool_schemas, true)
            .map_err(provider_error_from_llm)?;
        let url = format!("{}/api/chat", self.base_url);
        let request_bytes = serde_json::to_string(&request_body)
            .map(|value| value.len())
            .unwrap_or(0);
        let context = HttpAttemptContext {
            driver: "ollama",
            api: "chat_stream",
            provider: "ollama",
            model: scope.model(),
            method: "POST",
            endpoint: &url,
            attempt: 1,
            max_attempts: 1,
            message_count: messages.len(),
            tool_count: tool_schemas.len(),
            request_bytes,
        };
        let response = HttpAttemptExecutor::execute(
            self.http
                .post(&url)
                .headers(self.build_headers().map_err(provider_error_from_llm)?)
                .json(&request_body),
            &context,
            cancel,
        )
        .await
        .map_err(|failure| {
            failure.log(AttemptDisposition::FinalFailure);
            provider_error_from_attempt(failure)
        })?
        .response;
        Ok(crate::adapters::stream::invocation_stream_from_decoder(
            response,
            scope.effective_reasoning(),
            cancel.child_token(),
            crate::adapters::stream::InvocationDecoder::Ollama,
        ))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }

    fn max_reasoning_level(&self) -> crate::ports::ReasoningLevel {
        crate::ports::ReasoningLevel::Medium
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    async fn spawn_counting_server(raw_response: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let observed = counter.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                observed.fetch_add(1, Ordering::SeqCst);
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buffer = [0_u8; 8192];
                let _ = socket.read(&mut buffer).await;
                let _ = socket.write_all(raw_response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{addr}"), counter)
    }

    #[tokio::test]
    async fn llm_client_ollama_invocation_stream_is_single_request_pull_stream() {
        let body = concat!(
            "{\"message\":{\"role\":\"assistant\",\"content\":\"ol\"},\"done\":false}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"lama\"},\"done\":false}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":1}\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let leaked = Box::leak(response.into_boxed_str());
        let (base_url, requests) = spawn_counting_server(leaked).await;
        let client =
            crate::composition::LlmClient::from_config(crate::composition::LlmConfigOptions {
                driver: crate::ProviderDriverKind::Ollama.as_str().to_string(),
                source_key: "ollama".to_string(),
                api_style: None,
                api_key: "ollama".to_string(),
                base_url: Some(base_url),
                model: "test-model".to_string(),
                max_tokens: 8192,
                reasoning: false,
                reasoning_config: None,
                timeout_secs: 60,
            })
            .expect("valid ollama config");
        let scope = InvocationScope::new(
            "test-model",
            8192,
            crate::ReasoningLevel::Off,
            crate::ReasoningLevel::Off,
        )
        .unwrap();

        let events: Vec<_> = client
            .invocation_stream(
                &scope,
                &[],
                &[Message::user("hi")],
                &[],
                &CancellationToken::new(),
            )
            .await
            .unwrap()
            .collect()
            .await;

        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(matches!(
            &events[..],
            [
                crate::InvocationEvent::Delta(crate::InvocationDelta::Text(first)),
                crate::InvocationEvent::Delta(crate::InvocationDelta::Text(second)),
                crate::InvocationEvent::Completed(_)
            ] if first == "ol" && second == "lama"
        ));
        assert_eq!(events.iter().filter(|event| event.is_terminal()).count(), 1);
        let crate::InvocationEvent::Completed(completion) = events.last().unwrap() else {
            panic!("expected completed event");
        };
        let usage = completion.usage.as_ref().expect("ollama usage reported");
        assert_eq!(usage.input_tokens, Some(1));
        assert_eq!(usage.output_tokens, Some(1));
    }
}
