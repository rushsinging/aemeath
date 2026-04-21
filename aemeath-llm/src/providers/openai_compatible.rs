//! OpenAI-compatible provider implementation
//! Supports OpenAI, OpenRouter, DeepSeek, Moonshot, Zhipu, DashScope, and generic OpenAI-compatible APIs

use async_trait::async_trait;
use aemeath_core::message::{ContentBlock, Message, Role};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use std::error::Error as StdError;
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

use crate::provider::{LlmProvider, Provider, StreamHandler};
use crate::types::{StreamResponse, SystemBlock};

pub struct OpenAICompatibleProvider {
    provider: Provider,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    user_agent: String,
    http: reqwest::Client,
    /// Maximum retry attempts (default 3)
    max_retries: u32,
    /// Request timeout in seconds (default 60)
    timeout_secs: u64,
    /// Whether this model uses reasoning/thinking mode
    reasoning: bool,
}

impl OpenAICompatibleProvider {
    pub fn new(
        provider: Provider,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
    ) -> Self {
        Self {
            provider,
            base_url: {
                let url = base_url.unwrap_or_else(|| provider.default_base_url().to_string());
                // Strip trailing /v1 to avoid double /v1/v1 when building request URL
                url.trim_end_matches('/').trim_end_matches("/v1").to_string()
            },
            model: model.unwrap_or_else(|| provider.default_model().to_string()),
            api_key,
            max_tokens,
            user_agent: format!("aemeath/{}", env!("CARGO_PKG_VERSION")),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to create HTTP client"),
            max_retries: 10,
            timeout_secs: 120,
            reasoning,
        }
    }

    /// Set maximum retry attempts
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set request timeout in seconds
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
        
        // Different providers use different header formats
        match self.provider {
            Provider::OpenRouter => {
                headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .map_err(|e| crate::LlmError::Config(e.to_string()))?);
                headers.insert("HTTP-Referer", HeaderValue::from_static("https://github.com/aemeath"));
            }
            _ => {
                headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .map_err(|e| crate::LlmError::Config(e.to_string()))?);
            }
        }
        
        headers.insert(USER_AGENT, HeaderValue::from_str(&self.user_agent)
            .map_err(|e| crate::LlmError::Config(e.to_string()))?);
        Ok(headers)
    }

    /// Convert Anthropic-style system blocks to OpenAI-style system message
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

    /// Convert messages from Anthropic format to OpenAI format
    fn convert_messages(&self, system: &[SystemBlock], messages: &[Message]) -> Result<Vec<serde_json::Value>, crate::LlmError> {
        let mut openai_messages = Vec::new();
        
        // Add system message if present
        if !system.is_empty() {
            openai_messages.push(Self::convert_system_to_message(system));
        }
        
        // Convert messages
        for msg in messages {
            let mut content_parts = Vec::new();
            let mut tool_calls: Vec<serde_json::Value> = Vec::new();
            
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        content_parts.push(serde_json::json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                    ContentBlock::Image { source } => {
                        match source {
                            aemeath_core::message::ImageSource::Base64 { media_type, data } => {
                                content_parts.push(serde_json::json!({
                                    "type": "image_url",
                                    "image_url": {
                                        "url": format!("data:{};base64,{}", media_type, data)
                                    }
                                }));
                            }
                        }
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        let args = serde_json::to_string(input)
                            .map_err(|e| crate::LlmError::Config(format!("Failed to serialize tool input: {}", e)))?;
                        tool_calls.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": args
                            }
                        }));
                    }
                    ContentBlock::ToolResult { tool_use_id, content, is_error } => {
                        // Tool result is a separate message in OpenAI format
                        openai_messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": match content {
                                serde_json::Value::String(s) => s.clone(),
                                serde_json::Value::Array(parts) => {
                                    parts.iter()
                                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("")
                                }
                                _ => content.to_string()
                            }
                        }));
                        
                        // If there's an error, we could add it to the content
                        if *is_error {
                            // Error is already included in content for most providers
                        }
                    }
                    ContentBlock::Thinking { .. } => {
                        // Thinking blocks are not supported in OpenAI format, skip
                    }
                }
            }
            
            // Skip the outer message if it only contained ToolResult blocks
            // (those were already emitted as individual "role":"tool" messages above)
            if content_parts.is_empty() && tool_calls.is_empty() {
                continue;
            }

            // Build the message
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };

            let mut message = serde_json::json!({
                "role": role
            });

            if !content_parts.is_empty() {
                if content_parts.len() == 1 && content_parts[0].get("type").and_then(|t| t.as_str()) == Some("text") {
                    message["content"] = content_parts[0]["text"].clone();
                } else {
                    message["content"] = serde_json::Value::Array(content_parts);
                }
            } else {
                message["content"] = serde_json::Value::Null;
            }

            if !tool_calls.is_empty() {
                message["tool_calls"] = serde_json::Value::Array(tool_calls);
            }

            openai_messages.push(message);
        }
        
        Ok(openai_messages)
    }

    /// Convert tool schemas from Anthropic format to OpenAI format
    fn convert_tools(tool_schemas: &[serde_json::Value]) -> Vec<serde_json::Value> {
        tool_schemas
            .iter()
            .filter_map(|schema| {
                // Anthropic format: { "name": "...", "description": "...", "input_schema": {...} }
                // OpenAI format: { "type": "function", "function": { "name": "...", "description": "...", "parameters": {...} } }
                let name = schema.get("name")?.as_str()?;
                let description = schema.get("description").and_then(|d| d.as_str()).unwrap_or("");
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

    async fn send_message_non_stream(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
    ) -> Result<StreamResponse, crate::LlmError> {
        let openai_messages = self.convert_messages(system, messages)?;
        let tools = Self::convert_tools(tool_schemas);
        
        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": openai_messages,
            "max_tokens": self.max_tokens,
            "stream": false
        });

        // Control reasoning/thinking mode based on config
        if !self.reasoning {
            request_body["enable_thinking"] = serde_json::json!(false);
        }

        if !tools.is_empty() {
            request_body["tools"] = serde_json::Value::Array(tools);
        }

        let headers = self.build_headers()?;

        let response = self
            .http
            .post(format!("{}{}", self.base_url, self.provider.chat_api_suffix()))
            .headers(headers)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| crate::LlmError::Network(e.to_string()))?;
        
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(crate::LlmError::Api {
                error_type: status.to_string(),
                message: body,
            });
        }
        
        let body: serde_json::Value = response.json().await
            .map_err(|e| crate::LlmError::Stream(e.to_string()))?;
        
        // Parse the response
        let mut content_blocks = Vec::new();
        let mut input_tokens = 0u32;
        let mut output_tokens = 0u32;
        let mut stop_reason = crate::types::StopReason::EndTurn;
        
        // Extract usage
        if let Some(usage) = body.get("usage") {
            input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            output_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        }
        
        // Extract content from choices
        if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
            if let Some(choice) = choices.first() {
                // Check finish_reason
                if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    stop_reason = match finish {
                        "stop" => crate::types::StopReason::EndTurn,
                        "tool_calls" => crate::types::StopReason::ToolUse,
                        "length" => crate::types::StopReason::MaxTokens,
                        _ => crate::types::StopReason::EndTurn,
                    };
                }
                
                if let Some(message) = choice.get("message") {
                    // Extract reasoning content (e.g. glm-5.1, DeepSeek-R1)
                    // Displayed separately via on_thinking, NOT added to content_blocks
                    if let Some(reasoning) = message.get("reasoning_content").and_then(|c| c.as_str()) {
                        if !reasoning.is_empty() {
                            handler.on_thinking(reasoning);
                        }
                    }

                    // Extract text content
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        if !content.is_empty() {
                            handler.on_text(content);
                            handler.on_text_block_complete(content);
                            content_blocks.push(ContentBlock::Text {
                                text: content.to_string(),
                            });
                        }
                    }
                    
                    // Extract tool calls
                    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                        for tool_call in tool_calls {
                            if let Some(function) = tool_call.get("function") {
                                let id = tool_call.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                                let name = function.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                                let arguments = function.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}");
                                let input: serde_json::Value = serde_json::from_str(arguments)
                                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                                
                                handler.on_tool_use_start(&name);
                                content_blocks.push(ContentBlock::ToolUse { id, name, input });
                            }
                        }
                    }
                }
            }
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
impl LlmProvider for OpenAICompatibleProvider {
    async fn stream_message(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let openai_messages = self.convert_messages(system, messages)?;
        let tools = Self::convert_tools(tool_schemas);
        
        let mut request_body = serde_json::json!({
            "model": self.model,
            "messages": openai_messages,
            "max_tokens": self.max_tokens,
            "stream": true,
            "stream_options": { "include_usage": true }
        });

        // Control reasoning/thinking mode based on config
        if !self.reasoning {
            request_body["enable_thinking"] = serde_json::json!(false);
        }

        if !tools.is_empty() {
            request_body["tools"] = serde_json::Value::Array(tools);
        }

        let headers = self.build_headers()?;

        let mut last_error = None;
        for attempt in 0..self.max_retries {
            if cancel.is_cancelled() {
                return Err(crate::LlmError::Stream("interrupted by user".to_string()));
            }
            
            if attempt > 0 {
                let delay = std::time::Duration::from_millis((1000 * 2u64.pow(attempt as u32)).min(30_000));
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
                .post(format!("{}{}", self.base_url, self.provider.chat_api_suffix()))
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
                            let url = format!("{}{}", self.base_url, self.provider.chat_api_suffix());
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
                            // Network errors are retryable — surface retry progress to UI
                            let remaining = self.max_retries.saturating_sub(attempt + 1);
                            if remaining > 0 {
                                handler.on_error(&format!(
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
            if status == 429 {
                let remaining = self.max_retries.saturating_sub(attempt + 1);
                if remaining > 0 {
                    handler.on_error(&format!(
                        "rate limited (429), retrying ({}/{})...",
                        attempt + 2, self.max_retries
                    ));
                }
                last_error = Some(crate::LlmError::RateLimited);
                continue;
            }

            // Retry 5xx errors (server-side issues may be transient)
            if status.as_u16() >= 500 && status.as_u16() < 600 {
                let error_body = response.text().await.unwrap_or_default();
                let remaining = self.max_retries.saturating_sub(attempt + 1);
                if remaining > 0 {
                    handler.on_error(&format!(
                        "server error ({}), retrying ({}/{})...",
                        status, attempt + 2, self.max_retries
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
                    // Streaming decode error — retry first, fallback to non-streaming on last attempt
                    handler.on_error(&format!("Streaming error: {}, retrying...", e));
                    last_error = Some(crate::LlmError::Stream(e));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        
        // All streaming retries exhausted — try one final non-streaming attempt
        if let Some(ref err) = last_error {
            if matches!(err, crate::LlmError::Stream(_)) {
                handler.on_error("All streaming retries failed, attempting non-streaming fallback");
                return self.send_message_non_stream(system, messages, tool_schemas, handler).await;
            }
        }
        Err(last_error.unwrap_or(crate::LlmError::Network("max retries exceeded".to_string())))
    }
    
    fn model_name(&self) -> &str {
        &self.model
    }
    
    fn provider_name(&self) -> &str {
        match self.provider {
            Provider::OpenAI => "openai",
            Provider::OpenRouter => "openrouter",
            Provider::DeepSeek => "deepseek",
            Provider::Moonshot => "moonshot",
            Provider::Zhipu => "zhipu",
            Provider::DashScope => "dashscope",
            Provider::MiniMax => "minimax",
            Provider::OpenAICompatible => "openai-compatible",
            Provider::Anthropic => "anthropic", // shouldn't happen but fallback
            Provider::Ollama => "ollama", // shouldn't happen — use OllamaProvider instead
        }
    }
}

/// Parse OpenAI-style SSE stream
async fn parse_openai_stream(
    response: reqwest::Response,
    handler: &mut dyn StreamHandler,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut current_tool_calls: std::collections::HashMap<usize, (String, String, String)> = std::collections::HashMap::new();
    let mut usage = crate::types::Usage { input_tokens: 0, output_tokens: 0 };
    let mut stop_reason = crate::types::StopReason::EndTurn;
    
    // Stream idle watchdog: abort if no chunks for 90s
    const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
    // Stall detection: log warning if gap between chunks > 30s
    const STALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(30);
    let mut last_event_time: Option<std::time::Instant> = None;

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
                handler.on_error(&format!("Stream idle timeout: no data for {}s", STREAM_IDLE_TIMEOUT.as_secs()));
                return Err(crate::LlmError::Stream(format!(
                    "Stream idle timeout: no data received for {}s", STREAM_IDLE_TIMEOUT.as_secs()
                )));
            }
            result = lines.next_line() => {
                match result.map_err(|e| crate::LlmError::Stream(e.to_string()))? {
                    Some(line) => line,
                    None => break,
                }
            }
        };

        // Stall detection
        let now = std::time::Instant::now();
        if let Some(last) = last_event_time {
            let gap = now.duration_since(last);
            if gap > STALL_THRESHOLD {
                // Stream stall detected — silently ignored
            }
        }
        last_event_time = Some(now);
        handler.on_raw_line(&line);
        
        // Parse SSE format
        let data = if line.starts_with("data: ") {
            &line[6..]
        } else if line.starts_with("data:") {
            &line[5..]
        } else {
            continue;
        };
        
        if data == "[DONE]" {
            break;
        }
        
        let chunk: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        
        // Check for error
        if let Some(error) = chunk.get("error") {
            let error_msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            handler.on_error(error_msg);
            return Err(crate::LlmError::Api {
                error_type: "api_error".to_string(),
                message: error_msg.to_string(),
            });
        }
        
        // Extract usage if present (some providers include it in the last chunk)
        if let Some(usage_obj) = chunk.get("usage") {
            if !usage_obj.is_null() {
                let in_tok = usage_obj.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let out_tok = usage_obj.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                if in_tok > 0 || out_tok > 0 {
                    usage.input_tokens = in_tok;
                    usage.output_tokens = out_tok;
                }
            }
        }
        
        // Process choices
        if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                // Check finish_reason
                if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    stop_reason = match finish {
                        "stop" => crate::types::StopReason::EndTurn,
                        "tool_calls" => crate::types::StopReason::ToolUse,
                        "length" => crate::types::StopReason::MaxTokens,
                        _ => crate::types::StopReason::EndTurn,
                    };
                }
                
                // Process delta
                if let Some(delta) = choice.get("delta") {
                    // Reasoning content (e.g. glm-5.1, DeepSeek-R1)
                    if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                        if !reasoning.is_empty() {
                            handler.on_thinking(reasoning);
                        }
                    }

                    // Text content
                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                        handler.on_text(content);
                        current_text.push_str(content);
                    }
                    
                    // Tool calls
                    if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                        for tc in tool_calls {
                            let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                            
                            // Get or create tool call entry
                            let entry = current_tool_calls.entry(index).or_insert_with(|| {
                                (String::new(), String::new(), String::new())
                            });
                            
                            // Update ID if present
                            if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                                entry.0 = id.to_string();
                            }
                            
                            // Update function info
                            if let Some(function) = tc.get("function") {
                                if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                                    entry.1 = name.to_string();
                                    if entry.0.is_empty() {
                                        // Some providers don't send tool call ID
                                        entry.0 = format!("call_{}", index);
                                    }
                                }
                                if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                                    entry.2.push_str(args);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Build final content blocks
    if !current_text.is_empty() {
        handler.on_text_block_complete(&current_text);
        content_blocks.push(ContentBlock::Text {
            text: current_text,
        });
    }
    
    // Sort tool calls by index and add to content
    let mut sorted_tool_calls: Vec<_> = current_tool_calls.into_iter().collect();
    sorted_tool_calls.sort_by_key(|(i, _)| *i);
    
    for (_, (id, name, arguments)) in sorted_tool_calls {
        if !name.is_empty() {
            handler.on_tool_use_start(&name);
            let input: serde_json::Value = serde_json::from_str(&arguments)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            content_blocks.push(ContentBlock::ToolUse { id, name, input });
        }
    }
    
    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
        },
        usage,
        stop_reason,
    })
}