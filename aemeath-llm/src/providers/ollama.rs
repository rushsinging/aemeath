//! Ollama provider implementation
//! Optimized for local Ollama inference server with longer timeouts,
//! optional auth, no stream_options, and empty response detection.

use async_trait::async_trait;
use aemeath_core::message::{ContentBlock, Message, Role};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

use crate::provider::{LlmProvider, StreamHandler};
use crate::types::{StreamResponse, SystemBlock};

pub struct OllamaProvider {
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    /// If false, send `think: false` to disable reasoning mode for models
    /// that support it (qwen3, deepseek-r1, gpt-oss, etc.)
    reasoning: bool,
    user_agent: String,
    http: reqwest::Client,
    max_retries: u32,
    timeout_secs: u64,
}

/// Default request timeout for Ollama (5 minutes) — model loading can be slow
const DEFAULT_TIMEOUT_SECS: u64 = 300;
/// Stream idle timeout: abort if no data for 3 minutes (Ollama may stall during generation)
const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

impl OllamaProvider {
    pub fn new(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
    ) -> Self {
        Self {
            base_url: {
                let url = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());
                url.trim_end_matches('/').trim_end_matches("/v1").to_string()
            },
            model: model.unwrap_or_else(|| "llama3.2".to_string()),
            api_key,
            max_tokens,
            reasoning,
            user_agent: format!("aemeath/{}", env!("CARGO_PKG_VERSION")),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self.http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(secs))
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

    /// Convert Anthropic-style system blocks to native ollama system message
    #[allow(dead_code)]
    fn convert_system_to_message(system: &[SystemBlock]) -> serde_json::Value {
        let system_text: String = system
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        serde_json::json!({
            "role": "system",
            "content": system_text
        })
    }

    /// Convert messages from Anthropic format to native ollama /api/chat format.
    ///
    /// Key differences vs OpenAI-compat:
    /// - Images live in a sibling `images: [base64]` array (no `data:` URL prefix)
    /// - Tool calls: `function.arguments` is a JSON **object**, not a string
    /// - Tool result: role `"tool"` with plain `content`; ollama correlates by order
    ///   (no `tool_call_id` / `tool_name` fields required)
    fn convert_messages(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
    ) -> Result<Vec<serde_json::Value>, crate::LlmError> {
        let mut ollama_messages = Vec::new();

        // Collect <system-reminder> content from leading user messages to merge
        // into the system message — Ollama models follow system instructions
        // much more reliably than user-message-wrapped XML tags.
        let mut system_extras: Vec<String> = Vec::new();
        let mut first_non_reminder = 0;
        for msg in messages {
            if msg.role != Role::User { break; }
            let all_text: String = msg.content.iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            if all_text.trim().starts_with("<system-reminder>") {
                system_extras.push(all_text);
                first_non_reminder += 1;
            } else {
                break;
            }
        }

        // Build system message: original system blocks + extracted reminders
        let mut system_parts: Vec<String> = system.iter()
            .map(|b| b.text.as_str().to_string())
            .collect();
        system_parts.extend(system_extras);

        if !system_parts.is_empty() {
            let system_text = system_parts.join("\n\n");
            ollama_messages.push(serde_json::json!({
                "role": "system",
                "content": system_text
            }));
        }

        // Process remaining messages (skip the leading system-reminder ones)
        for msg in &messages[first_non_reminder..] {
            let mut text_parts: Vec<String> = Vec::new();
            let mut images: Vec<String> = Vec::new();
            let mut tool_calls: Vec<serde_json::Value> = Vec::new();

            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        text_parts.push(text.clone());
                    }
                    ContentBlock::Image { source } => match source {
                        aemeath_core::message::ImageSource::Base64 {
                            media_type: _,
                            data,
                        } => {
                            // ollama native format: bare base64 string, no data: prefix
                            images.push(data.clone());
                        }
                    },
                    ContentBlock::ToolUse { id: _, name, input } => {
                        // Native format: arguments is a JSON object, not a string
                        tool_calls.push(serde_json::json!({
                            "function": {
                                "name": name,
                                "arguments": input
                            }
                        }));
                    }
                    ContentBlock::ToolResult {
                        tool_use_id: _,
                        content,
                        is_error: _,
                    } => {
                        let text = match content {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Array(parts) => parts
                                .iter()
                                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                .collect::<Vec<_>>()
                                .join(""),
                            _ => content.to_string(),
                        };
                        ollama_messages.push(serde_json::json!({
                            "role": "tool",
                            "content": text
                        }));
                    }
                    ContentBlock::Thinking { .. } => {
                        // Thinking blocks are internal; not re-sent to ollama
                    }
                }
            }

            if text_parts.is_empty() && images.is_empty() && tool_calls.is_empty() {
                continue;
            }

            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };

            let mut message = serde_json::json!({
                "role": role,
                "content": text_parts.join("")
            });

            if !images.is_empty() {
                message["images"] = serde_json::Value::Array(
                    images.into_iter().map(serde_json::Value::String).collect()
                );
            }

            if !tool_calls.is_empty() {
                message["tool_calls"] = serde_json::Value::Array(tool_calls);
            }

            ollama_messages.push(message);
        }

        Ok(ollama_messages)
    }

    /// Convert tool schemas to native ollama format (same shape as OpenAI-compat)
    fn convert_tools(tool_schemas: &[serde_json::Value]) -> Vec<serde_json::Value> {
        tool_schemas
            .iter()
            .filter_map(|schema| {
                let name = schema.get("name")?.as_str()?;
                let description = schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                let input_schema = schema.get("input_schema")?;

                Some(serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": description,
                        "parameters": input_schema
                    }
                }))
            })
            .collect()
    }

    /// Build a native `/api/chat` request body. Shared between streaming
    /// and non-streaming paths; toggle `stream` accordingly.
    fn build_request_body(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        stream: bool,
    ) -> Result<serde_json::Value, crate::LlmError> {
        let ollama_messages = self.convert_messages(system, messages)?;
        let tools = Self::convert_tools(tool_schemas);

        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": ollama_messages,
            "stream": stream,
            // think toggles reasoning mode natively (qwen3, deepseek-r1, gpt-oss...)
            "think": self.reasoning,
        });

        // ollama uses `options.num_predict` for max tokens
        if self.max_tokens > 0 && self.max_tokens <= 128000 {
            request_body["options"] = serde_json::json!({
                "num_predict": self.max_tokens
            });
        }

        if !tools.is_empty() {
            request_body["tools"] = serde_json::Value::Array(tools);
        }

        Ok(request_body)
    }

    async fn send_message_non_stream(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
    ) -> Result<StreamResponse, crate::LlmError> {
        let request_body = self.build_request_body(system, messages, tool_schemas, false)?;
        let headers = self.build_headers()?;
        let url = format!("{}/api/chat", self.base_url);

        log::debug!(
            "[ollama non-stream] POST {} model={} think={} msgs={} tools={} body_bytes={}",
            url,
            self.model,
            self.reasoning,
            messages.len(),
            tool_schemas.len(),
            serde_json::to_string(&request_body).map(|s| s.len()).unwrap_or(0),
        );

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                let detail = if e.is_connect() {
                    "connection failed"
                } else if e.is_timeout() {
                    "request timed out"
                } else if e.is_request() {
                    "request build error"
                } else {
                    "unknown"
                };
                let mut msg = format!("{} ({})\n  URL: {}", e, detail, url);
                let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                let mut depth = 1;
                while let Some(cause) = source {
                    msg.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                    source = cause.source();
                    depth += 1;
                }
                crate::LlmError::Network(msg)
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(crate::LlmError::Api {
                error_type: status.to_string(),
                message: body,
            });
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| crate::LlmError::Stream(e.to_string()))?;

        let mut content_blocks = Vec::new();
        // ollama native usage: prompt_eval_count / eval_count at top level
        let input_tokens = body
            .get("prompt_eval_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = body
            .get("eval_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let mut stop_reason = crate::types::StopReason::EndTurn;

        if let Some(done_reason) = body.get("done_reason").and_then(|v| v.as_str()) {
            stop_reason = match done_reason {
                "stop" => crate::types::StopReason::EndTurn,
                "length" => crate::types::StopReason::MaxTokens,
                _ => crate::types::StopReason::EndTurn,
            };
        }

        if let Some(message) = body.get("message") {
            // Thinking (reasoning) content — native field is `thinking`
            if let Some(thinking) = message.get("thinking").and_then(|v| v.as_str()) {
                if !thinking.is_empty() {
                    handler.on_thinking(thinking);
                }
            }

            if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                if !content.is_empty() {
                    handler.on_text(content);
                    handler.on_text_block_complete(content);
                    content_blocks.push(ContentBlock::Text {
                        text: content.to_string(),
                    });
                }
            }

            if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                if !tool_calls.is_empty() {
                    stop_reason = crate::types::StopReason::ToolUse;
                }
                for (idx, tool_call) in tool_calls.iter().enumerate() {
                    if let Some(function) = tool_call.get("function") {
                        let id = tool_call
                            .get("id")
                            .and_then(|i| i.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("call_{}", idx));
                        let name = function
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        // Native format: arguments is already a JSON object
                        let input = function
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| {
                                serde_json::Value::Object(serde_json::Map::new())
                            });

                        handler.on_tool_use_start(&name);
                        content_blocks.push(ContentBlock::ToolUse { id, name, input });
                    }
                }
            }
        }

        if content_blocks.is_empty() {
            return Err(crate::LlmError::Stream(
                "Ollama returned empty response (no text or tool calls)".to_string(),
            ));
        }

        Ok(StreamResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: content_blocks,
            },
            usage: crate::types::Usage {
                input_tokens,
                output_tokens,
            },
            stop_reason,
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn stream_message(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let request_body = self.build_request_body(system, messages, tool_schemas, true)?;
        let headers = self.build_headers()?;
        let url = format!("{}/api/chat", self.base_url);

        let body_bytes = serde_json::to_string(&request_body).map(|s| s.len()).unwrap_or(0);
        log::debug!(
            "[ollama stream] POST {} model={} think={} msgs={} tools={} body_bytes={}",
            url,
            self.model,
            self.reasoning,
            messages.len(),
            tool_schemas.len(),
            body_bytes,
        );

        let mut last_error = None;
        for attempt in 0..self.max_retries {
            if cancel.is_cancelled() {
                return Err(crate::LlmError::Stream("interrupted by user".to_string()));
            }

            if attempt > 0 {
                let delay = std::time::Duration::from_millis((1000 * 2u64.pow(attempt as u32)).min(30_000));
                log::debug!(
                    "[ollama stream] retry {}/{} after {:?}",
                    attempt, self.max_retries, delay
                );
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
                .post(&url)
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
                        Ok(r) => r,
                        Err(e) => {
                            let msg = e.to_string();
                            if msg.contains("timed out") || msg.contains("timeout") {
                                // Ollama request timed out — will retry
                                last_error = Some(crate::LlmError::Network(format!(
                                    "Ollama request timed out after {}s — is the model loaded?", self.timeout_secs
                                )));
                                continue;
                            }
                            let mut detailed = format!("{}\n  URL: {}", e, url);
                            let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                            let mut depth = 1;
                            while let Some(cause) = source {
                                detailed.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                                source = cause.source();
                                depth += 1;
                            }
                            return Err(crate::LlmError::Network(detailed));
                        }
                    }
                }
            };

            let status = response.status();
            log::debug!(
                "[ollama stream] attempt={} HTTP {} content-type={:?}",
                attempt,
                status,
                response.headers().get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
            );

            if status == 429 {
                last_error = Some(crate::LlmError::RateLimited);
                continue;
            }

            if status.as_u16() >= 500 && status.as_u16() < 600 {
                let error_body = response.text().await.unwrap_or_default();
                log::debug!("[ollama stream] 5xx body: {}", error_body);
                last_error = Some(crate::LlmError::Api {
                    error_type: status.to_string(),
                    message: error_body,
                });
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                log::debug!("[ollama stream] non-success body: {}", body);
                return Err(crate::LlmError::Api {
                    error_type: status.to_string(),
                    message: body,
                });
            }

            match parse_ollama_stream(response, handler, cancel).await {
                Ok(resp) => {
                    // Check for empty response — Ollama sometimes returns valid stream
                    // with no actual content
                    if resp.assistant_message.content.is_empty() {
                        handler.on_error("Ollama stream returned no content, falling back to non-streaming");
                        return self
                            .send_message_non_stream(system, messages, tool_schemas, handler)
                            .await;
                    }
                    return Ok(resp);
                }
                Err(crate::LlmError::Stream(ref msg)) if msg.contains("interrupted") => {
                    return Err(crate::LlmError::Stream(msg.clone()));
                }
                Err(crate::LlmError::Stream(e)) => {
                    handler.on_error(&format!("Ollama streaming failed, falling back to non-streaming: {}", e));
                    return self
                        .send_message_non_stream(system, messages, tool_schemas, handler)
                        .await;
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_error.unwrap_or(crate::LlmError::Network(
            "Ollama: max retries exceeded".to_string(),
        )))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }
}

/// Parse ollama's native `/api/chat` NDJSON stream.
///
/// Stream format: one JSON object per line, no `data:` prefix, no `[DONE]`.
/// Each chunk: `{message:{role,content,thinking?,tool_calls?}, done, done_reason?, prompt_eval_count?, eval_count?}`.
/// Tool calls typically arrive in the final `done:true` chunk for qwen3-style models.
async fn parse_ollama_stream(
    response: reqwest::Response,
    handler: &mut dyn StreamHandler,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut final_tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
    let mut usage = crate::types::Usage {
        input_tokens: 0,
        output_tokens: 0,
    };
    let mut stop_reason = crate::types::StopReason::EndTurn;

    let byte_stream = response
        .bytes_stream()
        .map(|r| r.map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
    let reader = StreamReader::new(byte_stream);
    let mut lines = reader.lines();

    loop {
        let line = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(crate::LlmError::Stream("interrupted by user".to_string()));
            }
            _ = tokio::time::sleep(STREAM_IDLE_TIMEOUT) => {
                handler.on_error(&format!("Ollama stream idle timeout: no data for {}s", STREAM_IDLE_TIMEOUT.as_secs()));
                return Err(crate::LlmError::Stream(format!(
                    "Ollama stream idle timeout: no data for {}s — model may have stalled", STREAM_IDLE_TIMEOUT.as_secs()
                )));
            }
            result = lines.next_line() => {
                match result.map_err(|e| crate::LlmError::Stream(e.to_string()))? {
                    Some(line) => line,
                    None => break,
                }
            }
        };

        if line.trim().is_empty() {
            continue;
        }
        log::trace!("[ollama stream] <- {}", line);
        handler.on_raw_line(&line);

        // Native NDJSON: parse each non-empty line as a JSON object
        let chunk: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                log::debug!("[ollama stream] unparseable line ({}): {}", e, line);
                continue;
            }
        };

        // Stream-level error (ollama surfaces errors with an "error" key)
        if let Some(error) = chunk.get("error").and_then(|e| e.as_str()) {
            handler.on_error(error);
            return Err(crate::LlmError::Api {
                error_type: "ollama_error".to_string(),
                message: error.to_string(),
            });
        }

        if let Some(message) = chunk.get("message") {
            // Thinking delta
            if let Some(thinking) = message.get("thinking").and_then(|v| v.as_str()) {
                if !thinking.is_empty() {
                    handler.on_thinking(thinking);
                }
            }

            // Content delta
            if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    handler.on_text(content);
                    current_text.push_str(content);
                }
            }

            // Tool calls (typically in the final done:true chunk)
            if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                for (idx, tc) in tool_calls.iter().enumerate() {
                    if let Some(function) = tc.get("function") {
                        let id = tc
                            .get("id")
                            .and_then(|i| i.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("call_{}", idx));
                        let name = function
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = function
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| {
                                serde_json::Value::Object(serde_json::Map::new())
                            });
                        if !name.is_empty() {
                            final_tool_calls.push((id, name, input));
                        }
                    }
                }
            }
        }

        // Final chunk: done=true carries usage + done_reason
        if chunk.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
            if let Some(reason) = chunk.get("done_reason").and_then(|v| v.as_str()) {
                stop_reason = match reason {
                    "stop" => crate::types::StopReason::EndTurn,
                    "length" => crate::types::StopReason::MaxTokens,
                    _ => crate::types::StopReason::EndTurn,
                };
            }
            if let Some(n) = chunk.get("prompt_eval_count").and_then(|v| v.as_u64()) {
                usage.input_tokens = n as u32;
            }
            if let Some(n) = chunk.get("eval_count").and_then(|v| v.as_u64()) {
                usage.output_tokens = n as u32;
            }
            // Tool calls override the stop reason
            if !final_tool_calls.is_empty() {
                stop_reason = crate::types::StopReason::ToolUse;
            }
            break;
        }
    }

    let text_len = current_text.len();
    let tool_count = final_tool_calls.len();

    // Build final content blocks
    if !current_text.is_empty() {
        handler.on_text_block_complete(&current_text);
        content_blocks.push(ContentBlock::Text {
            text: current_text,
        });
    }

    for (id, name, input) in final_tool_calls {
        handler.on_tool_use_start(&name);
        content_blocks.push(ContentBlock::ToolUse { id, name, input });
    }

    log::debug!(
        "[ollama stream] done text_bytes={} tool_calls={} stop={:?} in_tok={} out_tok={}",
        text_len, tool_count, stop_reason, usage.input_tokens, usage.output_tokens
    );

    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
        },
        usage,
        stop_reason,
    })
}
