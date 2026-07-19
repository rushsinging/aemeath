use super::ask_user::AskUserQuestionTool;
use crate::domain::{ToolSuspension, TypedTool};

#[test]
fn ask_user_parses_validated_input_directly_into_pure_suspension() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({
            "question": "Choose one",
            "options": ["A", {"title": "B", "description": "second"}],
            "allow_free_input": false,
            "multi_select": true,
            "default": "A"
        }))
        .expect("AskUser always uses suspension")
        .expect("valid input");

    let ToolSuspension::UserInteraction(spec) = result;
    assert_eq!(spec.questions.len(), 1);
    assert_eq!(spec.questions[0].prompt, "Choose one");
    assert_eq!(spec.questions[0].options[0].title, "A");
    assert_eq!(spec.questions[0].options[0].description, None);
    assert_eq!(spec.questions[0].options[1].title, "B");
    assert_eq!(
        spec.questions[0].options[1].description.as_deref(),
        Some("second")
    );
    assert!(spec.questions[0].allow_multi);
    assert!(!spec.questions[0].allow_free_input);
    assert_eq!(spec.questions[0].default.as_deref(), Some("A"));
}

#[test]
fn ask_user_output_schema_matches_runtime_answer_payload() {
    let schema = AskUserQuestionTool.data_schema();
    assert_eq!(schema["properties"]["text"]["type"], "string");
    assert_eq!(schema["required"], serde_json::json!(["text"]));
    assert!(schema["properties"].get("question_type").is_none());
}

#[test]
fn ask_user_rejects_empty_question_without_runtime_state() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({"question": ""}))
        .expect("AskUser always uses suspension");
    assert_eq!(result.unwrap_err(), "Question is required");
}
