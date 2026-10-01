//! Anthropic message conversion helpers

use crate::adapters::constants::ANTHROPIC_TOOL_ALLOWED_KEYS;
use share::message::{ContentBlock, Message, Role};

// ---------------------------------------------------------------------------
// Tool schema sanitize — strip internal-only fields (data_schema etc.)
// before sending to the Anthropic Messages API. Only spec-allowed keys
// survive: name, description, input_schema, cache_control, type.
// ---------------------------------------------------------------------------

/// 将内部 tool schema（含 `data_schema` 等扩展字段）清洗为 Anthropic
/// Messages API 兼容格式，只保留白名单字段。
pub(crate) fn sanitize_tool_schemas(tool_schemas: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let empty = serde_json::Map::new();
    tool_schemas
        .iter()
        .map(|schema| {
            let obj = schema.as_object().unwrap_or(&empty);
            let filtered: serde_json::Map<String, serde_json::Value> = obj
                .iter()
                .filter(|(k, _)| ANTHROPIC_TOOL_ALLOWED_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            serde_json::Value::Object(filtered)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Message conversion — explicitly build Anthropic Messages API JSON from
// internal Message, avoiding serde_json::to_value leaks (metadata,
// placeholder, text etc. are internal-only and must never reach the wire).
// ---------------------------------------------------------------------------

/// 将内部 `Message` 列表转换为 Anthropic Messages API 兼容的 JSON 数组。
///
/// 显式遍历每个 `ContentBlock`，按 API 规范构建 wire format，丢弃所有
/// 内部扩展字段（`metadata`、`Image.placeholder`、`ToolResult.text`）。
/// 与 OpenAI 兼容 driver 的 `convert_messages` 对齐——各 driver 负责自己
/// 的 wire format。
pub(crate) fn convert_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    messages.iter().map(convert_message).collect()
}

/// 在 messages 倒数第二条消息的最后一个 content block 上注入
/// `cache_control: {"type": "ephemeral"}` 断点，让 Anthropic 缓存
/// 整个对话历史前缀。
///
/// Agentic loop 每 run step 新增 2 条消息，penultimate 消息不断前移，
/// 使前一 run 的缓存前缀与当前 run 的请求前缀匹配 → cache hit。
///
/// 配合 system static block（断点①）和 tools 数组（断点②），
/// 共使用 3/4 个 Anthropic 允许的 cache_control 断点。
pub(crate) fn apply_message_cache_breakpoint(messages: &mut [serde_json::Value]) {
    if messages.len() < 2 {
        return;
    }
    let penultimate = messages.len() - 2;
    if let Some(content) = messages[penultimate]
        .get_mut("content")
        .and_then(|c| c.as_array_mut())
    {
        // 倒序查找最后一个非 thinking block（Anthropic 不允许在 thinking
        // content block 上设置 cache_control，返回 400）
        for block in content.iter_mut().rev() {
            let is_thinking = block
                .get("type")
                .and_then(|t| t.as_str())
                .map(|s| s == "thinking" || s == "redacted_thinking")
                .unwrap_or(false);
            if !is_thinking {
                if let Some(obj) = block.as_object_mut() {
                    obj.insert(
                        "cache_control".to_string(),
                        serde_json::json!({"type": "ephemeral"}),
                    );
                }
                break;
            }
        }
    }
}

fn convert_message(msg: &Message) -> serde_json::Value {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<serde_json::Value> = msg
        .content
        .iter()
        .map(convert_block)
        .filter(|v| !v.is_null())
        .collect();
    serde_json::json!({
        "role": role,
        "content": content,
    })
}

fn convert_block(block: &ContentBlock) -> serde_json::Value {
    match block {
        ContentBlock::Text { text } => serde_json::json!({
            "type": "text",
            "text": text,
        }),
        ContentBlock::Image { source, .. } => {
            // 丢弃 placeholder（内部 round-trip 字段，不发给 LLM）
            match source {
                share::message::ImageSource::Base64 { media_type, data } => serde_json::json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": media_type,
                        "data": data,
                    },
                }),
            }
        }
        ContentBlock::ToolUse { id, name, input } => serde_json::json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            text,
        } => {
            // Anthropic 仅接受字符串或 content block 数组；优先使用 text-first 输出，
            // 并将旧会话中遗留的对象/标量 JSON 序列化为文本，避免非法请求出站。
            let content = text
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or_else(|| {
                    if content.is_object()
                        || content.is_number()
                        || content.is_boolean()
                        || content.is_null()
                    {
                        serde_json::Value::String(content.to_string())
                    } else {
                        content.clone()
                    }
                });
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error,
            })
        }
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            // Anthropic 要求后续请求中 thinking block 必须带 signature，
            // 否则返回 400 (`thinking.signature: Field required`)。
            // 有 signature → 回传；无 signature（旧 session / 非 Anthropic 来源）→ 剥离。
            match signature {
                Some(sig) => serde_json::json!({
                    "type": "thinking",
                    "thinking": thinking,
                    "signature": sig,
                }),
                None => serde_json::Value::Null,
            }
        }
    }
}

#[cfg(test)]
#[path = "message_conversion_tests.rs"]
mod tests;
