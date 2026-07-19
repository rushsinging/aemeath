//! Lossless Runtime seam from the Tool suspension PL to the existing SDK
//! interaction item. Runtime supplies the tool-call identity.

use sdk::{AskUserQuestionItem, OptionItem};

pub fn user_interaction_items(
    tool_call_id: &str,
    suspension: &tools::ToolSuspension,
) -> Vec<AskUserQuestionItem> {
    match suspension {
        tools::ToolSuspension::UserInteraction(spec) => spec
            .questions
            .iter()
            .map(|question| AskUserQuestionItem {
                id: tool_call_id.to_string(),
                question: question.prompt.clone(),
                options: question
                    .options
                    .iter()
                    .map(|option| OptionItem {
                        title: option.title.clone(),
                        description: option.description.clone(),
                    })
                    .collect(),
                multi_select: question.allow_multi,
                allow_free_input: question.allow_free_input,
                default: question.default.clone(),
            })
            .collect(),
    }
}
