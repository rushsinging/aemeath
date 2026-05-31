use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use share::message::Message;
use std::error::Error as StdError;
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;

use crate::business::types::SystemBlock;
use crate::core::provider::{LlmProvider, StreamHandler};

use super::{parse_openai_stream, OpenAICompatibleProvider, ReasoningConfig};

impl OpenAICompatibleProvider {
    pub(crate) fn base_request_body(
        &self,
        messages: Vec<serde_json::Value>,
        stream: bool,
    ) -> serde_json::Value {
        let max_tokens_field = self.driver.max_tokens_field();
        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            max_tokens_field: self.current_max_tokens(),
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

    pub(crate) fn apply_reasoning_fields(&self, request_body: &mut serde_json::Value) {
        let reasoning_enabled = self.reasoning.load(std::sync::atomic::Ordering::Relaxed);
        if let Ok(guard) = self.reasoning_config.lock() {
            self.driver
                .apply_reasoning_fields(request_body, guard.as_ref(), reasoning_enabled);
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    async fn stream_message(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<crate::business::types::StreamResponse, crate::LlmError> {
        let openai_messages = self.convert_messages(system, messages)?;
        let tools = Self::convert_tools(tool_schemas);

        let mut request_body = self.base_request_body(openai_messages, true);

        self.apply_reasoning_fields(&mut request_body);

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
            log::debug!(
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
                return Err(crate::LlmError::Stream("interrupted by user".to_string()));
            }

            if attempt > 0 {
                let delay =
                    std::time::Duration::from_millis((1000 * 2u64.pow(attempt)).min(30_000));
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        return Err(crate::LlmError::Stream("interrupted by user".to_string()));
                    }
                    _ = tokio::time::sleep(delay) => {}
                }
            }

            let send_fut = self
                .http
                .post(self.chat_url())
                .headers(headers.clone())
                .json(&request_body)
                .send();

            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Err(crate::LlmError::Stream("interrupted by user".to_string()));
                }
                result = send_fut => {
                    match result {
                        Ok(resp) => resp,
                        Err(e) => {
                            let url = self.chat_url();
                            let detail = if e.is_connect() {
                                "connection failed"
                            } else if e.is_timeout() {
                                "request timed out"
                            } else if e.is_redirect() {
                                "too many redirects"
                            } else if e.is_request() {
                                "request build error"
                            } else if e.is_body() {
                                "request body error"
                            } else if e.is_decode() {
                                "response decode error"
                            } else {
                                "unknown"
                            };
                            let mut msg = format!("{} ({})\n  URL: {}", e, detail, url);
                            let mut source: Option<&dyn StdError> = StdError::source(&e);
                            let mut depth = 1;
                            while let Some(cause) = source {
                                msg.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                                source = cause.source();
                                depth += 1;
                            }
                            let remaining = self.max_retries.saturating_sub(attempt + 1);
                            log::warn!(
                                "[openai-compat stream] HTTP send failed provider={} model={} attempt={}/{} remaining_retries={} detail={} body_bytes={} messages={} tools={} error={}",
                                self.config.source_key,
                                self.model,
                                attempt + 1,
                                self.max_retries,
                                remaining,
                                detail,
                                request_body_bytes,
                                request_message_count,
                                request_tool_count,
                                msg,
                            );
                            if remaining > 0 {                                handler.on_error(&format!(
                                    "network error ({detail}), retrying ({}/{})...",
                                    attempt + 2, self.max_retries
                                ));
                            }
                            last_error = Some(crate::LlmError::Network(msg));
                            continue;
                        }
                    }
                }
            };

            let status = response.status();
            log::debug!(
                "[openai-compat stream] response received provider={} model={} status={} attempt={}/{} body_bytes={} messages={} tools={}",
                self.config.source_key,
                self.model,
                status,
                attempt + 1,
                self.max_retries,
                request_body_bytes,
                request_message_count,
                request_tool_count,
            );
            if status == 429 {
                let remaining = self.max_retries.saturating_sub(attempt + 1);
                if remaining > 0 {
                    handler.on_error(&format!(
                        "rate limited (429), retrying ({}/{})...",
                        attempt + 2,
                        self.max_retries
                    ));
                }
                last_error = Some(crate::LlmError::RateLimited);
                continue;
            }

            if status.as_u16() >= 500 && status.as_u16() < 600 {
                let error_body = response.text().await.unwrap_or_default();
                let remaining = self.max_retries.saturating_sub(attempt + 1);
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
                    message: error_body,
                });
                continue;
            }

            if status == 413 {
                return Err(crate::LlmError::ContextTooLong);
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(crate::LlmError::Api {
                    error_type: status.to_string(),
                    message: body,
                });
            }

            match parse_openai_stream(response, handler, cancel).await {
                Ok(resp) => return Ok(resp),
                Err(crate::LlmError::Stream(ref msg)) if msg.contains("interrupted") => {
                    return Err(crate::LlmError::Stream(msg.clone()));
                }
                Err(crate::LlmError::Stream(e)) => {
                    let mut source_chain = String::new();
                    let stream_error = crate::LlmError::Stream(e.clone());
                    let mut source: Option<&dyn StdError> = StdError::source(&stream_error);
                    let mut depth = 1;
                    while let Some(cause) = source {
                        source_chain.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                        source = cause.source();
                        depth += 1;
                    }
                    log::warn!(
                        "[openai-compat stream] streaming parse failed provider={} model={} attempt={}/{} remaining_retries={} body_bytes={} messages={} tools={} error={}{}",
                        self.config.source_key,
                        self.model,
                        attempt + 1,
                        self.max_retries,
                        self.max_retries.saturating_sub(attempt + 1),
                        request_body_bytes,
                        request_message_count,
                        request_tool_count,
                        e,
                        source_chain,
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
                    .send_message_non_stream(system, messages, tool_schemas, handler)
                    .await;
            }
        }
        Err(last_error.unwrap_or(crate::LlmError::Network("max retries exceeded".to_string())))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.config.source_key
    }

    fn set_reasoning(&self, enabled: bool) {
        self.reasoning
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut guard) = self.reasoning_config.lock() {
            if matches!(*guard, Some(ReasoningConfig::Bool(_))) {
                *guard = Some(ReasoningConfig::Bool(enabled));
            }
        }
    }

    fn is_reasoning(&self) -> bool {
        self.reasoning.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_reasoning_effort(&self, effort: Option<String>) {
        if let Ok(mut guard) = self.reasoning_config.lock() {
            *guard = effort
                .map(|effort| ReasoningConfig::Object(serde_json::json!({ "effort": effort })));
        }
    }

    fn reasoning_effort(&self) -> Option<String> {
        self.reasoning_config
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(ReasoningConfig::as_effort))
    }

    fn set_max_tokens(&self, max_tokens: u32) {
        if max_tokens > 0 {
            self.max_tokens.store(max_tokens, Ordering::Relaxed);
        }
    }

    fn max_tokens(&self) -> u32 {
        self.current_max_tokens()
    }
}
