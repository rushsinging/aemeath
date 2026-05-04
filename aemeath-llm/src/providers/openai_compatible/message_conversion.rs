//! 消息格式转换：将 Anthropic 风格的消息转换为 OpenAI 格式

use super::OpenAICompatibleProvider;
use crate::types::SystemBlock;
use aemeath_core::message::{ContentBlock, Message, Role};

/// 在 OpenAI 消息序列发送前的最后一致性检查 + 自动修复。
///
/// OpenAI 严格要求：带 `tool_calls` 的 assistant 消息后**必须紧跟**与每个
/// `tool_call_id` 一一对应的 `role: "tool"` 消息。任何不一致都会导致
/// 400 "insufficient tool messages following tool_calls message"。
///
/// 这个函数是发请求前的最后一道闸：
/// - 如果某条 assistant 带 N 个 tool_calls，但紧跟的 tool messages 不覆盖
///   全部 id，就地插入占位 tool 消息补齐
/// - 任何 tool message 的 tool_call_id 找不到对应 assistant.tool_calls.id 时
///   将其移除（孤儿）
fn enforce_openai_tool_pairs(messages: &mut Vec<serde_json::Value>) {
    use std::collections::HashSet;

    // Step 1: 收集所有 assistant 发出过的 tool_call_id
    let mut all_call_ids: HashSet<String> = HashSet::new();
    for m in messages.iter() {
        if m.get("role").and_then(|r| r.as_str()) == Some("assistant") {
            if let Some(tcs) = m.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tcs {
                    if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                        all_call_ids.insert(id.to_string());
                    }
                }
            }
        }
    }

    // Step 2: 移除孤儿 tool 消息（其 tool_call_id 不属于任何 assistant.tool_calls）
    let before = messages.len();
    messages.retain(|m| {
        if m.get("role").and_then(|r| r.as_str()) == Some("tool") {
            let tcid = m.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
            if !all_call_ids.contains(tcid) {
                log::warn!("[openai-compat] dropping orphan tool message id={}", tcid);
                return false;
            }
        }
        true
    });
    if messages.len() != before {
        log::warn!(
            "[openai-compat] dropped {} orphan tool messages",
            before - messages.len()
        );
    }

    // Step 3: 对每条带 tool_calls 的 assistant，检查紧跟的 tool messages 是否覆盖全部 id；
    //         缺哪个就立即在它后面插入占位
    let mut i = 0;
    while i < messages.len() {
        let pending: Vec<String> = messages[i]
            .get("role")
            .and_then(|r| r.as_str())
            .filter(|r| *r == "assistant")
            .and_then(|_| messages[i].get("tool_calls").and_then(|t| t.as_array()))
            .map(|tcs| {
                tcs.iter()
                    .filter_map(|tc| tc.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        if pending.is_empty() {
            i += 1;
            continue;
        }

        // 收集紧跟 i 的连续 tool messages 已覆盖哪些 id
        let mut covered: HashSet<String> = HashSet::new();
        let mut last_tool_idx = i;
        let mut j = i + 1;
        while j < messages.len() && messages[j].get("role").and_then(|r| r.as_str()) == Some("tool")
        {
            if let Some(id) = messages[j].get("tool_call_id").and_then(|v| v.as_str()) {
                covered.insert(id.to_string());
            }
            last_tool_idx = j;
            j += 1;
        }

        // 缺失的 id：插入占位 tool 消息
        let missing: Vec<&String> = pending.iter().filter(|id| !covered.contains(*id)).collect();
        if !missing.is_empty() {
            log::warn!(
                "[openai-compat] assistant at index {} has {} tool_calls but only {} are answered. Inserting {} placeholder tool message(s).",
                i, pending.len(), covered.len(), missing.len()
            );
            let insert_after = last_tool_idx;
            for (offset, mid) in missing.iter().enumerate() {
                messages.insert(
                    insert_after + 1 + offset,
                    serde_json::json!({
                        "role": "tool",
                        "tool_call_id": mid,
                        "content": "[result missing — auto-filled to satisfy tool_calls schema]"
                    }),
                );
            }
            i = insert_after + 1 + missing.len();
        } else {
            i = last_tool_idx + 1;
        }
    }
}

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
                        aemeath_core::message::ImageSource::Base64 { media_type, data } => {
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
            if content_parts.is_empty() && tool_calls.is_empty() && reasoning_content.is_none() {
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

#[cfg(test)]
mod tests {
    use crate::providers::openai_compatible::{
        ApiDriverKind, OpenAICompatibleProvider, OpenAIProviderConfig, ReasoningConfig,
    };
    use aemeath_core::message::{ContentBlock, Message, Role};

    fn provider_with_reasoning() -> OpenAICompatibleProvider {
        OpenAICompatibleProvider::new(
            OpenAIProviderConfig::from_api_driver(ApiDriverKind::OpenAI, "DeepSeek"),
            "test-key".to_string(),
            None,
            Some("deepseek-v4-pro".to_string()),
            8192,
            true,
            Some(ReasoningConfig::Bool(true)),
        )
    }

    fn provider_without_reasoning() -> OpenAICompatibleProvider {
        OpenAICompatibleProvider::new(
            OpenAIProviderConfig::from_api_driver(ApiDriverKind::OpenAI, "DeepSeek"),
            "test-key".to_string(),
            None,
            Some("deepseek-v4-pro".to_string()),
            8192,
            false,
            Some(ReasoningConfig::Bool(false)),
        )
    }

    #[test]
    fn test_convert_messages_preserves_real_reasoning_content_with_tool_calls() {
        let provider = provider_with_reasoning();
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "需要读取文件".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path":"/tmp/a"}),
                },
            ],
        }];

        let converted = provider.convert_messages(&[], &messages).unwrap();
        let assistant = converted
            .iter()
            .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
            .unwrap();

        assert_eq!(
            assistant.get("reasoning_content"),
            Some(&serde_json::json!("需要读取文件"))
        );
        assert!(assistant.get("tool_calls").is_some());
    }

    #[test]
    fn test_convert_messages_preserves_real_reasoning_content_even_when_reasoning_disabled() {
        let provider = provider_without_reasoning();
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "已有推理内容".to_string(),
                },
                ContentBlock::Text {
                    text: "结论".to_string(),
                },
            ],
        }];

        let converted = provider.convert_messages(&[], &messages).unwrap();
        let assistant = converted
            .iter()
            .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
            .unwrap();

        assert_eq!(
            assistant.get("reasoning_content"),
            Some(&serde_json::json!("已有推理内容"))
        );
        assert_eq!(assistant.get("content"), Some(&serde_json::json!("结论")));
    }

    #[test]
    fn test_convert_messages_omits_reasoning_content_when_reasoning_disabled() {
        let provider = provider_without_reasoning();
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path":"/tmp/a"}),
            }],
        }];

        let converted = provider.convert_messages(&[], &messages).unwrap();
        let assistant = converted
            .iter()
            .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("assistant"))
            .unwrap();

        assert!(assistant.get("reasoning_content").is_none());
        assert!(assistant.get("tool_calls").is_some());
    }
}
