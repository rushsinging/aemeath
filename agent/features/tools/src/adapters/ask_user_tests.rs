use super::ask_user::AskUserQuestionTool;
use crate::domain::{ToolSuspension, TypedTool};

#[test]
fn ask_user_parses_validated_input_directly_into_pure_suspension() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({
            "question": "Choose one",
            "options": [
                {"title": "A", "description": "first"},
                {"title": "B", "description": "second"}
            ],
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
    assert_eq!(spec.questions[0].options[0].description, "first");
    assert_eq!(spec.questions[0].options[1].title, "B");
    assert_eq!(spec.questions[0].options[1].description, "second");
    assert!(spec.questions[0].allow_multi);
    assert!(!spec.questions[0].allow_free_input);
    assert_eq!(spec.questions[0].default.as_deref(), Some("A"));
}

#[test]
fn ask_user_rejects_plain_string_options() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({
            "question": "Choose one",
            "options": ["A", "B"]
        }))
        .expect("AskUser always uses suspension");

    let error = result.expect_err("plain string options must be rejected");
    assert!(
        error.contains("object"),
        "error should explain object form is required: {error}"
    );
}

#[test]
fn ask_user_rejects_option_without_description() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({
            "question": "Choose one",
            "options": [{"title": "A"}]
        }))
        .expect("AskUser always uses suspension");

    let error = result.expect_err("option without description must be rejected");
    assert!(
        error.contains("description"),
        "error should name the missing field: {error}"
    );
}

#[test]
fn ask_user_rejects_option_without_title() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({
            "question": "Choose one",
            "options": [{"description": "first"}]
        }))
        .expect("AskUser always uses suspension");

    let error = result.expect_err("option without title must be rejected");
    assert!(
        error.contains("title"),
        "error should name the missing field: {error}"
    );
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

#[test]
fn ask_user_schema_describes_default_free_input_and_builtin_option() {
    let schema = AskUserQuestionTool.input_schema();
    let description = schema["properties"]["allow_free_input"]["description"]
        .as_str()
        .expect("allow_free_input description");

    assert!(description.contains("Defaults to true"));
    assert!(description.contains("Type something..."));
}

#[test]
fn ask_user_schema_requires_object_options_with_description() {
    let schema = AskUserQuestionTool.input_schema();
    let description = schema["properties"]["options"]["description"]
        .as_str()
        .expect("options description");

    assert!(description.contains("object"));
    assert!(description.contains("description"));
    assert!(description.contains("required"));
}

#[test]
fn ask_user_parses_multiple_questions_in_one_call() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({
            "questions": [
                {
                    "question": "First question",
                    "options": [
                        {"title": "A", "description": "first"},
                        {"title": "B", "description": "second"}
                    ],
                    "multi_select": true
                },
                {
                    "question": "Second question",
                    "options": [{"title": "C", "description": "third"}],
                    "allow_free_input": false,
                    "default": "C"
                }
            ]
        }))
        .expect("AskUser always uses suspension")
        .expect("multi-question input must parse");

    let ToolSuspension::UserInteraction(spec) = result;
    assert_eq!(spec.questions.len(), 2);
    assert_eq!(spec.questions[0].prompt, "First question");
    assert_eq!(spec.questions[0].options.len(), 2);
    assert!(spec.questions[0].allow_multi);
    assert!(spec.questions[0].allow_free_input);
    assert_eq!(spec.questions[1].prompt, "Second question");
    assert_eq!(spec.questions[1].options[0].title, "C");
    assert!(!spec.questions[1].allow_multi);
    assert!(!spec.questions[1].allow_free_input);
    assert_eq!(spec.questions[1].default.as_deref(), Some("C"));
}

#[test]
fn ask_user_rejects_empty_questions_array() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({ "questions": [] }))
        .expect("AskUser always uses suspension");

    let error = result.expect_err("empty questions must be rejected");
    assert!(
        error.contains("questions"),
        "error should name the questions field: {error}"
    );
}

#[test]
fn ask_user_rejects_both_single_question_and_questions() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({
            "question": "Single form",
            "questions": [{"question": "Array form"}]
        }))
        .expect("AskUser always uses suspension");

    let error = result.expect_err("mixing both forms must be rejected");
    assert!(
        error.contains("questions"),
        "error should name the questions field: {error}"
    );
}

#[test]
fn ask_user_rejects_question_inside_questions_without_text() {
    let result = AskUserQuestionTool
        .suspension(&serde_json::json!({
            "questions": [{"question": ""}]
        }))
        .expect("AskUser always uses suspension");

    let error = result.expect_err("empty question inside questions must be rejected");
    assert!(
        error.contains("Question is required"),
        "error should keep the single-question wording: {error}"
    );
}

#[test]
fn ask_user_schema_exposes_questions_array_with_item_fields() {
    let schema = AskUserQuestionTool.input_schema();
    let questions = &schema["properties"]["questions"];

    assert_eq!(questions["type"], "array");
    let item = &questions["items"];
    assert_eq!(item["type"], "object");
    assert_eq!(
        item["properties"]["question"]["type"], "string",
        "each item carries its own question field"
    );
    assert!(item["properties"].get("options").is_some());
    assert!(item["properties"].get("multi_select").is_some());
    assert!(item["properties"].get("allow_free_input").is_some());
}
