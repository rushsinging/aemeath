//! 消息格式转换：将 Anthropic 风格的消息转换为 OpenAI 格式

use super::{message_helpers::enforce_openai_tool_pairs, OpenAICompatibleProvider};
use crate::business::types::SystemBlock;
use share::message::{ContentBlock, Message, Role};

impl OpenAICompatibleProvider {
    /// 将 Anthropic 风格的 system 块转换为 OpenAI 风格的 system 消息
    pub(crate) fn convert_system_to_message(system: &[SystemBlock]) -> serde_json::Value {
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

    /// 将消息从 Anthropic 格式转换为 OpenAI 格式
    pub(crate) fn convert_messages(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
    ) -> Result<Vec<serde_json::Value>, crate::LlmError> {
        let mut openai_messages = Vec::new();

        // 如果存在 system 消息则添加
        if !system.is_empty() {
            openai_messages.push(Self::convert_system_to_message(system));
        }

        // 转换消息
        for msg in messages {
            let mut content_parts = Vec::new();
            let mut tool_calls: Vec<serde_json::Value> = Vec::new();
            let mut reasoning_content: Option<String> = None;

            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        content_parts.push(serde_json::json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                    ContentBlock::Image { source } => match source {
                        share::message::ImageSource::Base64 { media_type, data } => {
                            content_parts.push(serde_json::json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:{};base64,{}", media_type, data)
                                }
                            }));
                        }
                    },
                    ContentBlock::ToolUse { id, name, input } => {
                        let args = serde_json::to_string(input).map_err(|e| {
                            crate::LlmError::Config(format!(
                                "Failed to serialize tool input: {}",
                                e
                            ))
                        })?;
                        tool_calls.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": args
                            }
                        }));
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        ..
                    } => {
                        // Tool result 在 OpenAI 格式中是独立的消息
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

                        // 如果有错误，已包含在 content 中
                        if *is_error {
                            // 错误已包含在大多数 provider 的 content 中
                        }
                    }
                    ContentBlock::Thinking { thinking } => {
                        // DeepSeek-R1 / thinking 模式要求在下一轮完整回传
                        // `reasoning_content`，否则触发 HTTP 400。其他不识别此字段的
                        // provider 会忽略它，因此始终包含是安全的。
                        reasoning_content = Some(match reasoning_content.take() {
                            Some(existing) => existing + thinking,
                            None => thinking.clone(),
                        });
                    }
                }
            }

            // 如果外层消息只包含 ToolResult 块则跳过
            // （它们已经作为独立的 "role":"tool" 消息发出）
            if content_parts.is_empty() && tool_calls.is_empty() {
                continue;
            }

            // 构建消息
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };

            let mut message = serde_json::json!({
                "role": role
            });

            if !content_parts.is_empty() {
                if content_parts.len() == 1
                    && content_parts[0].get("type").and_then(|t| t.as_str()) == Some("text")
                {
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

            // 在 thinking 模式开启时，为每条 assistant 消息附加 reasoning_content。
            // DeepSeek 拒绝省略该字段的历史记录（"thinking 模式下的
            // `reasoning_content` 必须回传给 API"），即使模型未输出推理内容的轮次也是如此。
            // 空字符串可以接受。其他 OpenAI 兼容 provider 会静默忽略未知字段。
            if msg.role == Role::Assistant
                && (reasoning_content.is_some()
                    || self.reasoning.load(std::sync::atomic::Ordering::Relaxed))
            {
                let rc = reasoning_content.unwrap_or_default();
                message["reasoning_content"] = serde_json::Value::String(rc);
            }

            openai_messages.push(message);
        }

        // 最后一道防线：确保 OpenAI 协议的 tool_calls ↔ tool messages 一一对应。
        // 不论上游 compact / sanitize 路径有没有漏网之鱼，这里都会自动补救。
        enforce_openai_tool_pairs(&mut openai_messages);

        Ok(openai_messages)
    }

    /// 将工具 schema 从 Anthropic 格式转换为 OpenAI 格式
    pub(crate) fn convert_tools(tool_schemas: &[serde_json::Value]) -> Vec<serde_json::Value> {
        tool_schemas
            .iter()
            .filter_map(|schema| {
                // Anthropic 格式: { "name": "...", "description": "...", "input_schema": {...} }
                // OpenAI 格式: { "type": "function", "function": { "name": "...", "description": "...", "parameters": {...} } }
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
}
