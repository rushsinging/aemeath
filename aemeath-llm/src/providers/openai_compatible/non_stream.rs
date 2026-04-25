//! 非流式请求：发送消息并等待完整响应

use aemeath_core::message::{ContentBlock, Message, Role};
use crate::provider::StreamHandler;
use crate::types::{StreamResponse, SystemBlock};
use super::OpenAICompatibleProvider;

impl OpenAICompatibleProvider {
    pub(crate) async fn send_message_non_stream(
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

        // 根据 config 控制 reasoning/thinking 模式
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

        // 解析响应
        let mut content_blocks = Vec::new();
        let mut input_tokens = 0u32;
        let mut output_tokens = 0u32;
        let mut stop_reason = crate::types::StopReason::EndTurn;

        // 提取 usage
        if let Some(usage) = body.get("usage") {
            input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            output_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        }

        // 从 choices 中提取内容
        if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
            if let Some(choice) = choices.first() {
                // 检查 finish_reason
                if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    stop_reason = match finish {
                        "stop" => crate::types::StopReason::EndTurn,
                        "tool_calls" => crate::types::StopReason::ToolUse,
                        "length" => crate::types::StopReason::MaxTokens,
                        _ => crate::types::StopReason::EndTurn,
                    };
                }

                if let Some(message) = choice.get("message") {
                    // 提取 reasoning 内容（例如 glm-5.1, DeepSeek-R1）。
                    // 作为 Thinking 块保留在 content_blocks 中，以便下一轮
                    // convert_messages 可以将其作为 `reasoning_content` 字段重发——
                    // DeepSeek 的 thinking 模式拒绝省略此字段的 assistant 消息。
                    if let Some(reasoning) = message.get("reasoning_content").and_then(|c| c.as_str()) {
                        if !reasoning.is_empty() {
                            handler.on_thinking(reasoning);
                            content_blocks.push(ContentBlock::Thinking {
                                thinking: reasoning.to_string(),
                            });
                        }
                    }

                    // 提取文本内容
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        if !content.is_empty() {
                            handler.on_text(content);
                            handler.on_text_block_complete(content);
                            content_blocks.push(ContentBlock::Text {
                                text: content.to_string(),
                            });
                        }
                    }

                    // 提取 tool calls
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
