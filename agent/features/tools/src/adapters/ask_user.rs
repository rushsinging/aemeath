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
        "Ask the user a question and wait for their response. Use `options` array for predefined choices; never embed choices in the question text. Free-text input defaults to enabled; when options are present, the system supplies `Type something...` as its entry. Do not add that option yourself."
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
    if args.question.is_empty() {
        return Err("Question is required".to_string());
    }
    let options = args
        .options
        .unwrap_or_default()
        .into_iter()
        .filter_map(|option| {
            if let Some(title) = option.as_str() {
                return Some(UserOption::title_only(title));
            }
            let title = option.get("title")?.as_str()?.to_owned();
            let description = option
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Some(UserOption::new(title, description))
        })
        .collect();
    Ok(UserInteractionSpec::new(vec![UserQuestion::new(
        args.question,
        options,
        args.multi_select.unwrap_or(false),
        args.allow_free_input.unwrap_or(true),
        args.default,
    )]))
}
