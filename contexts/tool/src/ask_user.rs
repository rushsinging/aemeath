use async_trait::async_trait;
use kernel::tool::{Tool, ToolContext, ToolResult};
use serde_json::Value;

pub struct AskUserQuestionTool;

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn description(&self) -> &str {
        "Ask the user a question and wait for their response. Use this when you need clarification, confirmation, or user input to proceed with a task. When offering predefined choices, MUST put each choice in the options array as a separate item and keep question to the prompt text only. NEVER embed choices such as A/B/C, 1/2/3, or multiple selectable alternatives directly in question."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question prompt only. Do not include selectable choices here; put choices in options."
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of predefined answer choices. For multiple-choice questions, each choice MUST be one separate array item. Do not combine choices into one string or embed them in question."
                },
                "allow_free_input": {
                    "type": "boolean",
                    "description": "If true, user can provide any answer (not limited to options)"
                },
                "default": {
                    "type": "string",
                    "description": "Optional default answer if user skips"
                }
            },
            "required": ["question"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let question = input["question"].as_str().unwrap_or("");

        if question.is_empty() {
            return ToolResult::error("Question is required");
        }

        // 构建提示消息
        let options: Vec<String> = input["options"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let allow_free_input = input["allow_free_input"].as_bool().unwrap_or(true);
        let default = input["default"].as_str();

        let mut prompt = question.to_string();

        if !options.is_empty() {
            prompt.push_str("\n\nOptions:\n");
            for (i, opt) in options.iter().enumerate() {
                prompt.push_str(&format!("{}. {}\n", i + 1, opt));
            }
            if allow_free_input {
                prompt.push_str("\nYou can also provide a custom answer.");
            }
        }

        if let Some(default_val) = default {
            prompt.push_str(&format!("\n(Default: {})", default_val));
        }

        // 这里需要实际的 UI 层来实现用户输入
        // 由于当前在 tool 层，我们返回一个需要用户响应的提示
        // CLI 层应该处理这个交互

        // 使用取消令牌来检测是否被中断
        if ctx.cancel.is_cancelled() {
            return ToolResult::error("Question cancelled by user");
        }

        // 返回特殊格式的结果，让 CLI 层知道需要用户输入
        // 格式: __ASK_USER__: question
        let response = if !options.is_empty() && !allow_free_input {
            ToolResult::success(format!(
                "__ASK_USER_SELECT__: {}\nOptions: {}",
                question,
                options.join(",")
            ))
        } else {
            ToolResult::success(format!("__ASK_USER__: {}", question))
        };

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_description_requires_choices_in_options() {
        let tool = AskUserQuestionTool;
        let description = tool.description();

        assert!(description.contains("MUST put each choice in the options array"));
        assert!(description.contains("keep question to the prompt text only"));
        assert!(description.contains("NEVER embed choices"));
    }

    #[test]
    fn test_input_schema_question_description_rejects_embedded_choices() {
        let tool = AskUserQuestionTool;
        let schema = tool.input_schema();
        let description = schema["properties"]["question"]["description"]
            .as_str()
            .expect("question description should be a string");

        assert!(description.contains("question prompt only"));
        assert!(description.contains("Do not include selectable choices"));
        assert!(description.contains("put choices in options"));
    }

    #[test]
    fn test_input_schema_options_description_requires_separate_items() {
        let tool = AskUserQuestionTool;
        let schema = tool.input_schema();
        let description = schema["properties"]["options"]["description"]
            .as_str()
            .expect("options description should be a string");

        assert!(description.contains("each choice MUST be one separate array item"));
        assert!(description.contains("Do not combine choices into one string"));
        assert!(description.contains("embed them in question"));
    }
}
