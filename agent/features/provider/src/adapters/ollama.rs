//! Ollama provider implementation — 主模块
//! 本地 Ollama 推理服务优化：更长超时、可选认证、无 stream_options、空响应检测。

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
    HttpFailureKind, NetworkFailureKind,
};
use crate::domain::invoke::{InvocationScope, StreamResponse, SystemBlock};
use crate::ports::{LegacyStreamSink, LlmProvider};
use crate::LOG_TARGET;

mod conversion;
mod non_stream;
pub(crate) mod stream;

use conversion::OllamaProviderConversion;
use non_stream::OllamaProviderNonStream;
use stream::parse_ollama_stream;

pub struct OllamaProvider {
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) user_agent: String,
    pub(crate) http: reqwest::Client,
    pub(crate) max_retries: u32,
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
            max_retries: 10,
            timeout_secs,
        }
    }

    // 重试次数 / 超时为可选构建器旋钮，尚无配置来源接线；保留以备后续从
    // ModelRuntimeSettings 注入（refs #85）。
    #[allow(dead_code)]
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
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
        crate::LlmError::StreamTruncated { .. } => crate::ProviderErrorKind::StreamTruncated,
        crate::LlmError::Stream(_) => crate::ProviderErrorKind::Protocol,
        crate::LlmError::Config(_) => crate::ProviderErrorKind::Configuration,
    };
    crate::ProviderError::fatal(kind, error.to_string())
}

fn provider_error_from_attempt(failure: HttpAttemptFailure) -> crate::ProviderError {
    match failure {
        HttpAttemptFailure::Cancelled => crate::ProviderError::cancelled(),
        HttpAttemptFailure::Network { source, kind, .. } => crate::ProviderError::fatal(
            match kind {
                NetworkFailureKind::Timeout => crate::ProviderErrorKind::Timeout,
                _ => crate::ProviderErrorKind::Network,
            },
            source.to_string(),
        ),
        HttpAttemptFailure::Http {
            status, kind, body, ..
        } => {
            let error_kind = match kind {
                HttpFailureKind::RateLimited => crate::ProviderErrorKind::RateLimited,
                HttpFailureKind::ContextTooLong => crate::ProviderErrorKind::ContextTooLong,
                HttpFailureKind::Server => crate::ProviderErrorKind::UpstreamUnavailable,
                HttpFailureKind::Client => crate::ProviderErrorKind::InvalidRequest,
            };
            let mut error = crate::ProviderError::fatal(error_kind, body.text());
            error.provider_code = Some(status.to_string());
            error
        }
    }
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
        Ok(
            crate::adapters::stream::invocation_stream_from_legacy_decoder(
                response,
                scope.effective_reasoning(),
                cancel.child_token(),
                crate::adapters::stream::LegacyStreamDecoder::Ollama,
            ),
        )
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

    async fn legacy_stream_message(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn LegacyStreamSink,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let request_body = self.build_request_body(scope, system, messages, tool_schemas, true)?;
        let headers = self.build_headers()?;
        let url = format!("{}/api/chat", self.base_url);

        let body_bytes = serde_json::to_string(&request_body)
            .map(|s| s.len())
            .unwrap_or(0);
        log::debug!(target: LOG_TARGET,
            "[ollama stream] POST {} model={} think={} msgs={} tools={} body_bytes={}",
            url,
            scope.model(),
            scope.effective_reasoning() != crate::ports::ReasoningLevel::Off,
            messages.len(),
            tool_schemas.len(),
            body_bytes,
        );

        let mut last_error = None;
        for attempt in 0..self.max_retries {
            if cancel.is_cancelled() {
                return Err(crate::LlmError::Cancelled);
            }

            if attempt > 0 {
                let delay =
                    std::time::Duration::from_millis((1000 * 2u64.pow(attempt)).min(30_000));
                log::debug!(target: LOG_TARGET,
                    "[ollama stream] retry {}/{} after {:?}",
                    attempt,
                    self.max_retries,
                    delay
                );
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        return Err(crate::LlmError::Cancelled);
                    }
                    _ = tokio::time::sleep(delay) => {}
                }
            }

            let context = HttpAttemptContext {
                driver: "ollama",
                api: "chat_stream",
                provider: "ollama",
                model: scope.model(),
                method: "POST",
                endpoint: &url,
                attempt: attempt + 1,
                max_attempts: self.max_retries,
                message_count: messages.len(),
                tool_count: tool_schemas.len(),
                request_bytes: body_bytes,
            };

            let response = match HttpAttemptExecutor::execute(
                self.http
                    .post(&url)
                    .headers(headers.clone())
                    .json(&request_body),
                &context,
                cancel,
            )
            .await
            {
                Ok(success) => success.response,
                Err(failure) => {
                    let remaining = self.max_retries.saturating_sub(attempt + 1);
                    // Disposition mirrors the actual control-flow decision
                    // below, decided *after* typed classification — only
                    // `NetworkFailureKind::Timeout` and the retryable HTTP
                    // kinds ever loop back for another attempt; every other
                    // network kind and Client/ContextTooLong are terminal
                    // regardless of remaining budget.
                    let disposition = match &failure {
                        HttpAttemptFailure::Cancelled => AttemptDisposition::FinalFailure,
                        HttpAttemptFailure::Network { kind, .. } => match kind {
                            NetworkFailureKind::Timeout => {
                                AttemptDisposition::from_remaining(remaining)
                            }
                            _ => AttemptDisposition::FinalFailure,
                        },
                        HttpAttemptFailure::Http { kind, .. } => match kind {
                            HttpFailureKind::RateLimited | HttpFailureKind::Server => {
                                AttemptDisposition::from_remaining(remaining)
                            }
                            HttpFailureKind::ContextTooLong | HttpFailureKind::Client => {
                                AttemptDisposition::FinalFailure
                            }
                        },
                    };
                    // 单次记录：typed 分类决定 disposition 后，消费式
                    // failure.log(disposition) 只记一次，反映真实终态。
                    failure.log(disposition);
                    match failure {
                        HttpAttemptFailure::Cancelled => {
                            return Err(crate::LlmError::Cancelled);
                        }
                        HttpAttemptFailure::Network { source, kind, .. } => match kind {
                            NetworkFailureKind::Timeout => {
                                if remaining > 0 {
                                    handler.on_error(&format!(
                                        "Ollama request timed out, retrying ({}/{})...",
                                        attempt + 2,
                                        self.max_retries
                                    ));
                                }
                                last_error = Some(crate::LlmError::Network(format!(
                                    "Ollama request timed out after {}s — is the model loaded?",
                                    self.timeout_secs
                                )));
                                continue;
                            }
                            _ => {
                                let mut msg = format!("{}\n  URL: {}", source, url);
                                let mut cause: Option<&dyn std::error::Error> =
                                    std::error::Error::source(&source);
                                let mut depth = 1;
                                while let Some(c) = cause {
                                    msg.push_str(&format!("\n  Cause #{}: {}", depth, c));
                                    cause = c.source();
                                    depth += 1;
                                }
                                return Err(crate::LlmError::Network(msg));
                            }
                        },
                        HttpAttemptFailure::Http {
                            status, kind, body, ..
                        } => match kind {
                            HttpFailureKind::RateLimited => {
                                if remaining > 0 {
                                    handler.on_error(&format!(
                                        "rate limited ({}), retrying ({}/{})...",
                                        status,
                                        attempt + 2,
                                        self.max_retries
                                    ));
                                }
                                last_error = Some(crate::LlmError::RateLimited);
                                continue;
                            }
                            HttpFailureKind::ContextTooLong => {
                                return Err(crate::LlmError::ContextTooLong);
                            }
                            HttpFailureKind::Server => {
                                if remaining > 0 {
                                    handler.on_error(&format!(
                                        "server error ({}), retrying ({}/{})...",
                                        status,
                                        attempt + 2,
                                        self.max_retries
                                    ));
                                }
                                last_error = Some(crate::LlmError::Api {
                                    error_type: status.to_string(),
                                    message: body.text().to_string(),
                                });
                                continue;
                            }
                            HttpFailureKind::Client => {
                                return Err(crate::LlmError::Api {
                                    error_type: status.to_string(),
                                    message: body.text().to_string(),
                                });
                            }
                        },
                    }
                }
            };

            match parse_ollama_stream(response, handler, cancel).await {
                Ok(resp) => {
                    // Check for empty response — Ollama sometimes returns valid stream
                    // with no actual content
                    if resp.assistant_message.content.is_empty() {
                        handler.on_error(
                            "Ollama stream returned no content, falling back to non-streaming",
                        );
                        return self
                            .send_message_non_stream(
                                scope,
                                system,
                                messages,
                                tool_schemas,
                                handler,
                                cancel,
                            )
                            .await;
                    }
                    return Ok(resp);
                }
                Err(crate::LlmError::Stream(ref msg)) if msg.contains("interrupted") => {
                    return Err(crate::LlmError::Stream(msg.clone()));
                }
                Err(crate::LlmError::Stream(e)) => {
                    handler.on_error(&format!(
                        "Ollama streaming failed, falling back to non-streaming: {}",
                        e
                    ));
                    return self
                        .send_message_non_stream(
                            scope,
                            system,
                            messages,
                            tool_schemas,
                            handler,
                            cancel,
                        )
                        .await;
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_error.unwrap_or(crate::LlmError::Network(
            "Ollama: max retries exceeded".to_string(),
        )))
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
        let client = crate::LlmClient::from_config(crate::LlmConfigOptions {
            driver: crate::ProviderDriverKind::Ollama,
            api_key: "ollama".to_string(),
            base_url: Some(base_url),
            model: "test-model".to_string(),
            max_tokens: 8192,
            reasoning: false,
            reasoning_config: None,
            openai_config: None,
            timeout_secs: 60,
        });
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
    }
}
