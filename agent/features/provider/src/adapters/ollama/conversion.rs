//! 消息格式转换：将 Anthropic 风格转换为 Ollama 原生 /api/chat 格式。

use super::OllamaProvider;
use share::message::{ContentBlock, Message, Role};

/// 将转换方法封装为 trait，方便在 mod.rs 中通过 `self.convert_messages(...)` 调用。
pub(crate) trait OllamaProviderConversion {
    fn convert_messages(
        &self,
        system: &str,
        messages: &[Message],
    ) -> Result<Vec<serde_json::Value>, crate::LlmError>;
    fn convert_tools(tool_schemas: &[serde_json::Value]) -> Vec<serde_json::Value>;
    fn build_request_body(
        &self,
        resolved: &crate::ports::ResolvedInvocation,
        system: &str,
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
        system: &str,
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

        // Build system message: 整段 system prompt + extracted reminders
        // （#1861 v4：上游已拼接为整串，块级 join 消除）。
        let mut system_parts: Vec<String> = Vec::new();
        if !system.is_empty() {
            system_parts.push(system.to_string());
        }
        system_parts.extend(system_extras);

        if !system_parts.is_empty() {
            let system_text = system_parts.join("\n\n");
            ollama_messages.push(serde_json::json!({
                "role": "system",
                "content": system_text
            }));
        }

        // Process remaining messages (skip the leading system-reminder ones)
        // 数组索引切片（消息列表），非文本字节切片。
        // #1992：tool_result 图块累积；在下一批非 tool-only 消息（或末尾）前投递。
        let mut pending_tool_images: Vec<(String, String)> = Vec::new();
        for msg in &messages[first_non_reminder..] {
            // allow unsafe_text_op
            let mut text_parts: Vec<String> = Vec::new();
            let mut images: Vec<String> = Vec::new();
            let mut tool_calls: Vec<serde_json::Value> = Vec::new();

            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        text_parts.push(text.clone());
                    }
                    ContentBlock::Image { source, .. } => match source {
                        share::message::ImageSource::Base64 {
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
                        ..
                    } => {
                        // #1992：tool 消息不承载图片，图块摘出由后续 user 消息承接。
                        pending_tool_images.extend(
                            crate::adapters::tool_result_images::extract_tool_result_images(
                                content,
                            ),
                        );
                        let text =
                            crate::adapters::tool_result_images::extract_tool_result_text(content);
                        ollama_messages.push(serde_json::json!({
                            "role": "tool",
                            "content": text
                        }));
                    }
                    ContentBlock::Thinking { .. } => {
                        // Thinking blocks are internal; not re-sent to ollama
                    }
                    ContentBlock::Reminder {
                        kind,
                        version,
                        at,
                        seq,
                        body,
                    } => {
                        // 与 Text 同路径：渲染为 <system-reminder> XML envelope 后作为消息文本。
                        text_parts.push(ContentBlock::reminder_to_llm_envelope(
                            kind, *version, at, *seq, body,
                        ));
                    }
                }
            }

            if text_parts.is_empty() && images.is_empty() && tool_calls.is_empty() {
                continue;
            }

            // #1992：任何后续消息之前，先把已累积的 tool_result 图以 user
            // 消息投递（保证图消息紧随这批 tool 消息、不插入工具消息之间）。
            flush_tool_result_image_carrier(&mut ollama_messages, &mut pending_tool_images);

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

        // #1992：末尾残余（图来自最后一批 tool 消息）同样需要投递。
        flush_tool_result_image_carrier(&mut ollama_messages, &mut pending_tool_images);

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
        resolved: &crate::ports::ResolvedInvocation,
        system: &str,
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        stream: bool,
    ) -> Result<serde_json::Value, crate::LlmError> {
        let ollama_messages = self.convert_messages(system, messages)?;
        let tools = Self::convert_tools(tool_schemas);

        let mut request_body = serde_json::json!({
            "model": resolved.model.as_str(),
            "messages": ollama_messages,
            "stream": stream,
            // think toggles reasoning mode natively (qwen3, deepseek-r1, gpt-oss...)
            "think": resolved.effective_reasoning != crate::domain::capability::ReasoningLevel::Off,
        });

        // ollama uses `options.num_predict` for max tokens
        let max_tokens = resolved.max_tokens;
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

/// #1992：把累积的 tool_result 图块投递为一条 user 消息（native `images` 数组）。
///
/// 在下一个非 tool-only 消息之前、或遍历结束后调用；空列表为 no-op。
/// ollama native format 的 images 为裸 base64 字符串（无 `data:` 前缀）。
fn flush_tool_result_image_carrier(
    ollama_messages: &mut Vec<serde_json::Value>,
    pending_tool_images: &mut Vec<(String, String)>,
) {
    if pending_tool_images.is_empty() {
        return;
    }
    let images: Vec<serde_json::Value> = pending_tool_images
        .drain(..)
        .map(|(_, data)| serde_json::Value::String(data))
        .collect();
    ollama_messages.push(serde_json::json!({
        "role": "user",
        "content": "Images from the tool results above.",
        "images": serde_json::Value::Array(images)
    }));
}
