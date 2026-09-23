use crate::domain::types::ask_user::{AskUserQuestionInput, AskUserQuestionResult};
use crate::domain::{
    ToolExecutionContext, ToolSuspension, TypedTool, TypedToolResult, UserInteractionSpec,
    UserOption, UserQuestion,
};
use async_trait::async_trait;
use serde_json::Value;

pub struct AskUserQuestionTool;

#[async_trait]
impl TypedTool for AskUserQuestionTool {
    type Output = AskUserQuestionResult;
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn description(&self) -> &str {
        "Ask the user one or more questions and wait for their response. Use the `questions` array to ask multiple questions in one call — the user answers them in order and all answers are returned together; leave the top-level `question` empty in that case (never provide both). Use `options` array for predefined choices; never embed choices in the question text. Every option must be an object with required `title` and `description` fields; plain string options are rejected. Free-text input defaults to enabled; when options are present, the system supplies `Type something...` as its entry. Do not add that option yourself."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::ask_user(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        AskUserQuestionInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        AskUserQuestionResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        // Waits for user interaction and updates conversation flow state.
        false
    }

    fn suspension(&self, input: &Value) -> Option<Result<ToolSuspension, String>> {
        Some(ask_user_suspension(input))
    }

    async fn call(
        &self,
        _input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<AskUserQuestionResult> {
        TypedToolResult::error("AskUserQuestion must execute through the typed suspension seam")
    }
}

/// Compatibility seam for Runtime until the execution port is wired end to
/// end. It uses the same typed parser as `TypedTool::suspension` and owns no
/// Runtime identity or waiting state.
pub fn ask_user_suspension(input: &Value) -> Result<ToolSuspension, String> {
    parse_interaction(input).map(ToolSuspension::UserInteraction)
}

fn parse_interaction(input: &Value) -> Result<UserInteractionSpec, String> {
    let args: AskUserQuestionInput =
        serde_json::from_value(input.clone()).map_err(|error| format!("invalid input: {error}"))?;
    let questions = normalize_input_into_questions(args)?;
    Ok(UserInteractionSpec::new(questions))
}

/// 归一化两种输入形式（顶层单题字段 / `questions` 数组）到同一个
/// `Vec<UserQuestion>` 构建路径，单题与多题共享同一套校验。
fn normalize_input_into_questions(args: AskUserQuestionInput) -> Result<Vec<UserQuestion>, String> {
    let single_question_given = args
        .question
        .as_deref()
        .is_some_and(|question| !question.is_empty());
    let questions_array_given = args.questions.is_some();
    if single_question_given && questions_array_given {
        return Err(
            "provide either the top-level `question` or the `questions` array, not both"
                .to_string(),
        );
    }
    if questions_array_given {
        let items = args.questions.unwrap_or_default();
        if items.is_empty() {
            return Err("`questions` must contain at least one question".to_string());
        }
        return items
            .into_iter()
            .map(|item| {
                build_user_question(
                    item.question,
                    item.options,
                    item.multi_select,
                    item.allow_free_input,
                    item.default,
                )
            })
            .collect();
    }
    Ok(vec![build_user_question(
        args.question.unwrap_or_default(),
        args.options,
        args.multi_select,
        args.allow_free_input,
        args.default,
    )?])
}

fn build_user_question(
    question: String,
    options: Option<Vec<Value>>,
    multi_select: Option<bool>,
    allow_free_input: Option<bool>,
    default: Option<String>,
) -> Result<UserQuestion, String> {
    if question.is_empty() {
        return Err("Question is required".to_string());
    }
    let options = options
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(index, option)| {
            let position = format!("options[{index}]");
            if !option.is_object() {
                return Err(format!(
                    "{position}: each option must be an object with required `title` and `description` fields"
                ));
            }
            let title = option
                .get("title")
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
                .ok_or_else(|| format!("{position}: `title` is required and must be non-empty"))?;
            let description = option
                .get("description")
                .and_then(Value::as_str)
                .filter(|description| !description.is_empty())
                .ok_or_else(|| {
                    format!("{position}: `description` is required and must be non-empty")
                })?;
            Ok(UserOption::new(title, description))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(UserQuestion::new(
        question,
        options,
        multi_select.unwrap_or(false),
        allow_free_input.unwrap_or(true),
        default,
    ))
}
