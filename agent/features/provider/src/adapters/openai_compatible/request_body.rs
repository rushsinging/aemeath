use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use std::error::Error as StdError;
use tokio_util::sync::CancellationToken;

use crate::adapters::error_log;
use crate::adapters::http_attempt::{
    AttemptDisposition, HttpAttemptContext, HttpAttemptExecutor, HttpAttemptFailure,
    HttpFailureKind, NetworkFailureKind,
};
use crate::domain::invoke::{InvocationScope, SystemBlock};
use crate::ports::{LegacyStreamSink, LlmProvider, ReasoningLevel};
use crate::LOG_TARGET;

use super::{parse_openai_stream, OpenAICompatibleProvider, ReasoningConfig};

impl OpenAICompatibleProvider {
    pub(crate) async fn invoke_single_request_stream(
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
        let (request_body, url, api, decoder) = if self.config.use_responses_api {
            (
                self.build_responses_request_body(scope, system, messages, tool_schemas, true),
                self.responses_url(),
                "responses_stream",
                crate::adapters::stream::LegacyStreamDecoder::OpenAiResponses,
            )
        } else {
            let openai_messages = Self::convert_messages(
                system,
                messages,
                !matches!(scope.effective_reasoning(), ReasoningLevel::Off),
            )
            .map_err(provider_error_from_llm)?;
            let tools = Self::convert_tools(tool_schemas);
            let mut body = self.base_request_body(scope, openai_messages, true);
            self.apply_reasoning_fields(&mut body, scope);
            if !tools.is_empty() {
                body["tools"] = serde_json::Value::Array(tools);
                body["parallel_tool_calls"] = serde_json::Value::Bool(true);
            }
            (
                body,
                self.chat_url(),
                "chat_completions_stream",
                crate::adapters::stream::LegacyStreamDecoder::OpenAiChat,
            )
        };
        let request_bytes = serde_json::to_string(&request_body)
            .map(|value| value.len())
            .unwrap_or(0);
        let context = HttpAttemptContext {
            driver: "openai_compatible",
            api,
            provider: &self.config.source_key,
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
                decoder,
            ),
        )
    }

    pub(crate) fn base_request_body(
        &self,
        scope: &InvocationScope,
        messages: Vec<serde_json::Value>,
        stream: bool,
    ) -> serde_json::Value {
        let max_tokens_field = self.driver.max_tokens_field();
        let mut request_body = serde_json::json!({
            "model": scope.model(),
            "messages": messages,
            max_tokens_field: scope.max_tokens(),
            "stream": stream,
        });

        if stream {
            request_body["stream_options"] = serde_json::json!({ "include_usage": true });
        }

        request_body
    }

    pub(crate) fn build_headers(&self) -> Result<HeaderMap, crate::LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| crate::LlmError::Config(e.to_string()))?,
        );

        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&self.user_agent)
                .map_err(|e| crate::LlmError::Config(e.to_string()))?,
        );
        Ok(headers)
    }

    pub(crate) fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        scope: &InvocationScope,
    ) {
        let reasoning_enabled = !matches!(scope.effective_reasoning(), ReasoningLevel::Off);
        let scoped_config = self
            .reasoning_config
            .as_ref()
            .map(|config| config.for_scope(scope.effective_reasoning(), self.driver.as_ref()))
            .unwrap_or_else(|| {
                ReasoningConfig::from_scope(scope.effective_reasoning(), self.driver.as_ref())
            });
        self.driver
            .apply_reasoning_fields(request_body, Some(&scoped_config), reasoning_enabled);
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
impl LlmProvider for OpenAICompatibleProvider {
    async fn invocation_stream(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStream, crate::ProviderError> {
        self.invoke_single_request_stream(scope, system, messages, tool_schemas, cancel)
            .await
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.config.source_key
    }

    fn max_reasoning_level(&self) -> crate::ports::ReasoningLevel {
        self.driver.max_reasoning_level()
    }

    async fn legacy_stream_message(
        &self,
        scope: &InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn LegacyStreamSink,
        cancel: &CancellationToken,
    ) -> Result<crate::domain::invoke::StreamResponse, crate::LlmError> {
        // Responses API 分发（gpt-5.6-sol 等模型只支持 /v1/responses）
        if self.config.use_responses_api {
            return self
                .stream_message_responses(scope, system, messages, tool_schemas, handler, cancel)
                .await;
        }

        let openai_messages = Self::convert_messages(
            system,
            messages,
            !matches!(scope.effective_reasoning(), ReasoningLevel::Off),
        )?;
        let tools = Self::convert_tools(tool_schemas);

        let mut request_body = self.base_request_body(scope, openai_messages, true);

        self.apply_reasoning_fields(&mut request_body, scope);

        if !tools.is_empty() {
            request_body["tools"] = serde_json::Value::Array(tools);
            // Enable parallel tool calls so the model can return multiple
            // tool_use blocks in a single response, enabling true concurrent
            // execution of independent tasks (e.g. launching 6 reviewers at once).
            request_body["parallel_tool_calls"] = serde_json::Value::Bool(true);
        }

        if let Some(msgs) = request_body.get("messages").and_then(|m| m.as_array()) {
            let mut summary = String::with_capacity(256);
            for (i, m) in msgs.iter().enumerate() {
                let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("?");
                match role {
                    "assistant" => {
                        let has_tc = m.get("tool_calls").is_some();
                        let rc_len = m
                            .get("reasoning_content")
                            .and_then(|r| r.as_str())
                            .map(|s| s.len() as i32)
                            .unwrap_or(-1);
                        let content_null = m.get("content").map(|c| c.is_null()).unwrap_or(false);
                        summary.push_str(&format!(
                            "\n  [{i}] assistant rc_len={rc_len} tc={has_tc} content_null={content_null}"
                        ));
                    }
                    "tool" => {
                        let tcid = m.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
                        let tcid_short: String = tcid.chars().take(24).collect();
                        summary.push_str(&format!("\n  [{i}] tool id={tcid_short}"));
                    }
                    _ => {
                        summary.push_str(&format!("\n  [{i}] {role}"));
                    }
                }
            }
            let body_bytes = serde_json::to_string(&request_body)
                .map(|s| s.len())
                .unwrap_or(0);
            log::debug!(target: LOG_TARGET,
                "[openai-compat stream] POST provider={} body_bytes={} messages={}:{}",
                self.config.source_key,
                body_bytes,
                msgs.len(),
                summary,
            );
        }

        let headers = self.build_headers()?;
        let request_body_bytes = serde_json::to_string(&request_body)
            .map(|s| s.len())
            .unwrap_or(0);
        let request_message_count = request_body
            .get("messages")
            .and_then(|m| m.as_array())
            .map(Vec::len)
            .unwrap_or(0);
        let request_tool_count = request_body
            .get("tools")
            .and_then(|t| t.as_array())
            .map(Vec::len)
            .unwrap_or(0);

        let mut last_error = None;
        for attempt in 0..self.max_retries {
            if cancel.is_cancelled() {
                return Err(crate::LlmError::Cancelled);
            }

            if attempt > 0 {
                let delay =
                    std::time::Duration::from_millis((1000 * 2u64.pow(attempt)).min(30_000));
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        return Err(crate::LlmError::Cancelled);
                    }
                    _ = tokio::time::sleep(delay) => {}
                }
            }

            let context = HttpAttemptContext {
                driver: "openai_compatible",
                api: "chat_completions_stream",
                provider: &self.config.source_key,
                model: scope.model(),
                method: "POST",
                endpoint: &self.chat_url(),
                attempt: attempt + 1,
                max_attempts: self.max_retries,
                message_count: request_message_count,
                tool_count: request_tool_count,
                request_bytes: request_body_bytes,
            };

            let response = match HttpAttemptExecutor::execute(
                self.http
                    .post(self.chat_url())
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
                    // below, decided after typed classification — not a
                    // pre-guess. Client/ContextTooLong are unconditionally
                    // terminal regardless of remaining retry budget.
                    let disposition = match &failure {
                        HttpAttemptFailure::Cancelled => AttemptDisposition::FinalFailure,
                        HttpAttemptFailure::Network { .. } => {
                            AttemptDisposition::from_remaining(remaining)
                        }
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
                        HttpAttemptFailure::Network { source, kind, .. } => {
                            let detail = match kind {
                                NetworkFailureKind::Connect => "connection failed",
                                NetworkFailureKind::Timeout => "request timed out",
                                NetworkFailureKind::Redirect => "too many redirects",
                                NetworkFailureKind::Request => "request build error",
                                NetworkFailureKind::Body => "request body error",
                                NetworkFailureKind::Decode => "response decode error",
                                NetworkFailureKind::Unknown => "unknown",
                            };
                            let mut msg =
                                format!("{} ({})\n  URL: {}", source, detail, self.chat_url());
                            let mut cause: Option<&dyn StdError> = StdError::source(&source);
                            let mut depth = 1;
                            while let Some(c) = cause {
                                msg.push_str(&format!("\n  Cause #{}: {}", depth, c));
                                cause = c.source();
                                depth += 1;
                            }
                            log::debug!(target: LOG_TARGET,
                                "[openai-compat stream] HTTP send failed provider={} model={} attempt={}/{} remaining_retries={} detail={} body_bytes={} messages={} tools={} error={}",
                                self.config.source_key,
                                scope.model(),
                                attempt + 1,
                                self.max_retries,
                                remaining,
                                detail,
                                request_body_bytes,
                                request_message_count,
                                request_tool_count,
                                msg,
                            );
                            if remaining > 0 {
                                handler.on_error(&format!(
                                    "network error ({detail}), retrying ({}/{})...",
                                    attempt + 2,
                                    self.max_retries
                                ));
                            }
                            last_error = Some(crate::LlmError::Network(msg));
                            continue;
                        }
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

            log::debug!(target: LOG_TARGET,
                "[openai-compat stream] response received provider={} model={} attempt={}/{} body_bytes={} messages={} tools={}",
                self.config.source_key,
                scope.model(),
                attempt + 1,
                self.max_retries,
                request_body_bytes,
                request_message_count,
                request_tool_count,
            );

            match parse_openai_stream(response, handler, cancel).await {
                Ok(resp) => return Ok(resp),
                Err(crate::LlmError::Stream(ref msg)) if msg.contains("interrupted") => {
                    return Err(crate::LlmError::Stream(msg.clone()));
                }
                Err(crate::LlmError::Stream(e)) => {
                    let mut source_chain_text = String::new();
                    let stream_error = crate::LlmError::Stream(e.clone());
                    let mut source: Option<&dyn StdError> = StdError::source(&stream_error);
                    let mut depth = 1;
                    while let Some(cause) = source {
                        source_chain_text.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                        source = cause.source();
                        depth += 1;
                    }
                    let fallback_planned = e.contains("upstream truncated");
                    let remaining = self.max_retries.saturating_sub(attempt + 1);
                    // stream-protocol 错误不是 HTTP 层面的失败，HttpAttemptExecutor
                    // 不感知此阶段；使用 error_log 的窄 protocol 日志 API（而非手写
                    // 内部诊断结构体），这样 schema/JSON 解析
                    // 失败的诊断信息仍能落到 aemeath:llm-api-error。
                    let protocol_disposition = if fallback_planned || remaining == 0 {
                        AttemptDisposition::FallbackPlanned
                    } else {
                        AttemptDisposition::RetryPlanned
                    };
                    error_log::log_stream_protocol_error(
                        context.error_log_context(0),
                        &e,
                        protocol_disposition.retryable(),
                        fallback_planned,
                        protocol_disposition.log_level(),
                    );
                    log::debug!(target: LOG_TARGET,
                        "[openai-compat stream] streaming parse failed provider={} model={} attempt={}/{} remaining_retries={} body_bytes={} messages={} tools={} error={}{}",
                        self.config.source_key,
                        scope.model(),
                        attempt + 1,
                        self.max_retries,
                        self.max_retries.saturating_sub(attempt + 1),
                        request_body_bytes,
                        request_message_count,
                        request_tool_count,
                        e,
                        source_chain_text,
                    );
                    // SSE 流被上游截断是稳定性失败（不是瞬时网络抖动），
                    // 重试流式请求无法解决——直接跳出重试循环走 non-stream fallback。
                    if e.contains("upstream truncated") {
                        handler.on_error(&format!(
                            "Streaming error: {}, switching to non-streaming...",
                            e
                        ));
                        last_error = Some(crate::LlmError::Stream(e));
                        break;
                    }
                    handler.on_error(&format!("Streaming error: {}, retrying...", e));
                    last_error = Some(crate::LlmError::Stream(e));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        if let Some(ref err) = last_error {
            if matches!(err, crate::LlmError::Stream(_)) {
                handler.on_error("All streaming retries failed, attempting non-streaming fallback");
                return self
                    .send_message_non_stream(scope, system, messages, tool_schemas, handler, cancel)
                    .await;
            }
        }
        Err(last_error.unwrap_or(crate::LlmError::Network("max retries exceeded".to_string())))
    }
}
