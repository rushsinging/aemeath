//! 消息格式转换：将 Anthropic 风格转换为 Ollama 原生 /api/chat 格式。

use super::OllamaProvider;
use crate::types::SystemBlock;
use aemeath_core::message::{ContentBlock, Message, Role};

/// 将转换方法封装为 trait，方便在 mod.rs 中通过 `self.convert_messages(...)` 调用。
pub(crate) trait OllamaProviderConversion {
    fn convert_messages(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
    ) -> Result<Vec<serde_json::Value>, crate::LlmError>;
    fn convert_tools(tool_schemas: &[serde_json::Value]) -> Vec<serde_json::Value>;
    fn build_request_body(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        stream: bool,
    ) -> Result<serde_json::Value, crate::LlmError>;
}

impl OllamaProviderConversion for OllamaProvider {
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
            if msg.role != Role::User {
                break;
            }
            let all_text: String = msg
                .content
                .iter()
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
        let mut system_parts: Vec<String> =
            system.iter().map(|b| b.text.as_str().to_string()).collect();
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
                    images.into_iter().map(serde_json::Value::String).collect(),
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
            "think": self.reasoning.load(std::sync::atomic::Ordering::Relaxed),
        });

        // ollama uses `options.num_predict` for max tokens
        let max_tokens = self.current_max_tokens();
        if max_tokens > 0 && max_tokens <= 128000 {
            request_body["options"] = serde_json::json!({
                "num_predict": max_tokens
            });
        }

        if !tools.is_empty() {
            request_body["tools"] = serde_json::Value::Array(tools);
        }

        Ok(request_body)
    }
}
