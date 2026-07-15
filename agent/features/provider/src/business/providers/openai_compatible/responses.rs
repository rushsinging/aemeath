//! Responses API 请求构造（/v1/responses）
//!
//! 与 Chat Completions 的关键差异：
//! - `input` 替代 `messages`
//! - `max_output_tokens` 替代 `max_tokens`
//! - `reasoning: { effort }` 对象替代 `reasoning_effort` 字符串
//! - tools 扁平格式 `{ type:"function", name, description, parameters }`

use super::parse_responses_stream;
use super::OpenAICompatibleProvider;
use crate::business::error_log::{log_http_error, log_network_error, ErrorLogContext};
use crate::business::types::{StreamResponse, SystemBlock};
use crate::core::provider::StreamHandler;
use crate::LOG_TARGET;
use share::message::Message;
use tokio_util::sync::CancellationToken;

impl OpenAICompatibleProvider {
    /// Responses API streaming 入口
    pub(crate) async fn stream_message_responses(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let request_body = self.build_responses_request_body(system, messages, tool_schemas, true);
        let headers = self.build_headers()?;

        let request_body_bytes = serde_json::to_string(&request_body)
            .map(|s| s.len())
            .unwrap_or(0);
        let url = self.responses_url();

        log::debug!(target: LOG_TARGET,
            "[responses-stream] POST provider={} url={} body_bytes={}",
            self.config.source_key, url, request_body_bytes,
        );

        let invocation_started = std::time::Instant::now();
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
                    _ = cancel.cancelled() => return Err(crate::LlmError::Cancelled),
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
                _ = cancel.cancelled() => return Err(crate::LlmError::Cancelled),
                result = send_fut => match result {
                    Ok(resp) => resp,
                    Err(e) => {
                        let remaining = self.max_retries.saturating_sub(attempt + 1);
                        log_network_error(
                            ErrorLogContext {
                                driver: "openai_compatible",
                                api: "responses_stream",
                                provider: &self.config.source_key,
                                model: &self.model,
                                endpoint: &url,
                                attempt: attempt + 1,
                                max_attempts: self.max_retries,
                                elapsed_ms: invocation_started.elapsed().as_millis(),
                                message_count: messages.len(),
                                tool_count: tool_schemas.len(),
                                request_bytes: request_body_bytes,
                            },
                            &e,
                            remaining > 0,
                        );
                        log::debug!(target: LOG_TARGET,
                            "[responses-stream] HTTP send failed attempt={}/{}: {}",
                            attempt + 1, self.max_retries, e,
                        );
                        if attempt + 1 < self.max_retries {
                            handler.on_error(&format!("network error, retrying ({}/{})...", attempt + 2, self.max_retries));
                        }
                        last_error = Some(crate::LlmError::Network(e.to_string()));
                        continue;
                    }
                }
            };

            let status = response.status();
            log::debug!(target: LOG_TARGET,
                "[responses-stream] response status={} attempt={}/{}",
                status, attempt + 1, self.max_retries,
            );

            if status == 429 {
                let remaining = self.max_retries.saturating_sub(attempt + 1);
                if remaining > 0 {
                    handler.on_error(&format!(
                        "rate limited, retrying ({}/{})...",
                        attempt + 2,
                        self.max_retries
                    ));
                    last_error = Some(crate::LlmError::RateLimited);
                    continue;
                }
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                log_http_error(
                    ErrorLogContext {
                        driver: "openai_compatible",
                        api: "responses_stream",
                        provider: &self.config.source_key,
                        model: &self.model,
                        endpoint: &url,
                        attempt: attempt + 1,
                        max_attempts: self.max_retries,
                        elapsed_ms: invocation_started.elapsed().as_millis(),
                        message_count: messages.len(),
                        tool_count: tool_schemas.len(),
                        request_bytes: request_body_bytes,
                    },
                    status,
                    &body,
                    false,
                );
                return Err(crate::LlmError::Api {
                    error_type: status.to_string(),
                    message: body,
                });
            }

            return parse_responses_stream(response, handler, cancel).await;
        }

        Err(last_error.unwrap_or_else(|| crate::LlmError::Network("exhausted retries".to_string())))
    }

    /// 构造 Responses API 请求 body
    pub(crate) fn build_responses_request_body(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        stream: bool,
    ) -> serde_json::Value {
        // 将 system blocks 合并为 instructions
        let instructions: String = if system.is_empty() {
            String::new()
        } else {
            system
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n")
        };

        // 将 messages 转换为 input 格式
        let input = messages_to_responses_input(messages);

        let max_tokens = self.current_max_tokens().max(16);

        let mut body = serde_json::json!({
            "model": self.model,
            "input": input,
            "max_output_tokens": max_tokens,
            "stream": stream,
        });

        if !instructions.is_empty() {
            body["instructions"] = serde_json::Value::String(instructions);
        }

        // reasoning
        if self.reasoning.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(guard) = self.reasoning_config.lock() {
                let clamped = guard.as_ref().map(|c| c.clamped(self.driver.as_ref()));
                let effort = clamped
                    .as_ref()
                    .and_then(|c| c.as_effort())
                    .unwrap_or_else(|| "medium".to_string());
                body["reasoning"] = serde_json::json!({ "effort": effort });
            }
        }

        // tools（Responses API 扁平格式）
        if !tool_schemas.is_empty() {
            let tools: Vec<serde_json::Value> = tool_schemas
                .iter()
                .filter_map(|schema| {
                    let function = schema.get("function")?;
                    Some(serde_json::json!({
                        "type": "function",
                        "name": function.get("name").cloned().unwrap_or_default(),
                        "description": function.get("description").cloned().unwrap_or_default(),
                        "parameters": function.get("parameters").cloned().unwrap_or(serde_json::json!({})),
                    }))
                })
                .collect();
            if !tools.is_empty() {
                body["tools"] = serde_json::Value::Array(tools);
                body["parallel_tool_calls"] = serde_json::Value::Bool(true);
            }
        }

        if stream {
            body["stream_options"] = serde_json::json!({});
        }

        body
    }

    /// Responses API URL
    pub(crate) fn responses_url(&self) -> String {
        format!("{}/v1/responses", self.base_url)
    }
}

/// 将内部 Message 列表转为 Responses API input 格式。
///
/// Responses API 的 input 是一个 flat 数组，每个 item 有 `role` + `content`。
/// tool results 用 `{ type: "function_call_output", call_id, output }` 表示。
fn messages_to_responses_input(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut input = Vec::new();

    for msg in messages {
        match msg.role {
            share::message::Role::User => {
                // user message may contain text or tool results
                for block in &msg.content {
                    match block {
                        share::message::ContentBlock::Text { text } => {
                            input.push(serde_json::json!({
                                "type": "message",
                                "role": "user",
                                "content": text,
                            }));
                        }
                        share::message::ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            text,
                            ..
                        } => {
                            let output = text.clone().unwrap_or_else(|| match content {
                                serde_json::Value::String(s) => s.clone(),
                                _ => content.to_string(),
                            });
                            input.push(serde_json::json!({
                                "type": "function_call_output",
                                "call_id": tool_use_id,
                                "output": output,
                            }));
                        }
                        _ => {}
                    }
                }
            }
            share::message::Role::Assistant => {
                // 提取 text
                let text: String = msg
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        share::message::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !text.is_empty() {
                    input.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": text,
                    }));
                }
                // 提取 tool_use → function_call
                for block in &msg.content {
                    if let share::message::ContentBlock::ToolUse {
                        id,
                        name,
                        input: args,
                        ..
                    } = block
                    {
                        let args_str =
                            serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
                        input.push(serde_json::json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": args_str,
                        }));
                    }
                }
            }
        }
    }

    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_messages_to_responses_input_user() {
        let messages = vec![Message::user("hello")];
        let input = messages_to_responses_input(&messages);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "hello");
    }

    #[test]
    fn test_messages_to_responses_input_assistant_with_tool() {
        let messages = vec![Message {
            role: share::message::Role::Assistant,
            content: vec![
                share::message::ContentBlock::Text {
                    text: "thinking...".to_string(),
                },
                share::message::ContentBlock::ToolUse {
                    id: "call_123".to_string(),
                    name: "get_time".to_string(),
                    input: serde_json::json!({}),
                },
            ],
            metadata: None,
        }];
        let input = messages_to_responses_input(&messages);
        // text message + function_call
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["name"], "get_time");
        assert_eq!(input[1]["call_id"], "call_123");
    }

    #[test]
    fn test_messages_to_responses_input_tool_result() {
        let messages = vec![Message {
            role: share::message::Role::User,
            content: vec![share::message::ContentBlock::ToolResult {
                tool_use_id: "call_123".to_string(),
                content: serde_json::json!("12:00"),
                is_error: false,
                text: Some("12:00".to_string()),
            }],
            metadata: None,
        }];
        let input = messages_to_responses_input(&messages);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_123");
        assert_eq!(input[0]["output"], "12:00");
    }
}
