use super::*;
use crate::tui::model::input::completion_item::CompletionItem;

#[test]
fn test_suggestion_view_from_completion_maps_types_and_selection() {
    let mut completion = InputCompletion::default();
    completion.set_items(
        vec![CompletionItem::with_type(
            "src/main.rs",
            "src/main.rs",
            SuggestionType::File,
        )],
        "@src".to_string(),
    );

    let view = SuggestionViewState::from_completion(&completion);

    assert_eq!(view.selected, Some(0));
    assert_eq!(view.suggestions[0].display_text, "src/main.rs");
    assert!(matches!(
        view.suggestions[0].suggestion_type,
        SuggestionType::File
    ));
}

#[test]
fn test_suggestion_view_height_caps_visible_rows() {
    let mut completion = InputCompletion::default();
    completion.set_items(
        (0..8)
            .map(|i| CompletionItem::new(format!("/cmd{i}"), format!("/cmd{i}")))
            .collect(),
        "/".to_string(),
    );

    let view = SuggestionViewState::from_completion(&completion);

    assert_eq!(view.height(), 6);
}

#[test]
fn test_suggestion_view_empty_is_hidden() {
    let completion = InputCompletion::default();
    let view = SuggestionViewState::from_completion(&completion);

    assert!(!view.is_visible());
    assert_eq!(view.height(), 0);
}
