//! Responses API 请求构造（/v1/responses）
//!
//! 与 Chat Completions 的关键差异：
//! - `input` 替代 `messages`
//! - `max_output_tokens` 替代 `max_tokens`
//! - `reasoning: { effort }` 对象替代 `reasoning_effort` 字符串
//! - tools 扁平格式 `{ type:"function", name, description, parameters }`

use super::OpenAICompatibleProvider;
use crate::domain::invoke::{InvocationScope, SystemBlock};
use crate::ports::ReasoningLevel;
use share::message::Message;

impl OpenAICompatibleProvider {
    /// 构造 Responses API 请求 body
    pub(crate) fn build_responses_request_body(
        &self,
        scope: &InvocationScope,
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

        let max_tokens = scope.max_tokens().max(16);

        let mut body = serde_json::json!({
            "model": scope.model(),
            "input": input,
            "max_output_tokens": max_tokens,
            "stream": stream,
        });

        if !instructions.is_empty() {
            body["instructions"] = serde_json::Value::String(instructions);
        }

        // reasoning effort is resolved per invocation scope via the driver's shared
        // capability→wire mapping (resolve_effort), not a duplicated special case.
        if !matches!(scope.effective_reasoning(), ReasoningLevel::Off) {
            body["reasoning"] = serde_json::json!({
                "effort": self.driver.resolve_effort(scope.effective_reasoning())
            });
        }

        // tools（Responses API 扁平格式：{type:"function", name, description, parameters}）
        // ToolRegistry::schemas_for 产出 Anthropic 扁平格式 {name, description, input_schema}，
        // 这里直接取字段构建 Responses 格式（不要假设嵌套的 function 包装）。
        if !tool_schemas.is_empty() {
            let tools: Vec<serde_json::Value> = tool_schemas
                .iter()
                .filter_map(|schema| {
                    let name = schema.get("name")?.as_str()?;
                    let description = schema
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    let parameters = schema
                        .get("input_schema")
                        .cloned()
                        .unwrap_or(serde_json::json!({}));
                    Some(serde_json::json!({
                        "type": "function",
                        "name": name,
                        "description": description,
                        "parameters": parameters,
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
#[path = "responses_tests.rs"]
mod tests;
