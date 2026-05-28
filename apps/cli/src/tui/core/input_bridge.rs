use crate::tui::completion::Suggestion;
use crate::tui::model::input::change::InputChange;
use crate::tui::model::input::completion_item::CompletionItem;
#[cfg(test)]
use crate::tui::model::input::intent::InputIntent;
use crate::tui::{InputArea, StatusBar};

pub(crate) fn mirror_input_area_to_model(
    model: &mut crate::tui::model::root::TuiModel,
    input_area: &InputArea,
) {
    model.input.document.clear();
    model.input.document.insert_text(&input_area.get_text());
    let (_, col) = input_area.cursor_position();
    model.input.document.move_cursor(col);
    let (history_entries, history_index, saved_input) = input_area.history_snapshot();
    model.input.history.entries = history_entries.to_vec();
    model.input.history.selected_index = history_index;
    model.input.history.saved_input = saved_input.to_string();
    let (suggestions, selected_suggestion, show_suggestions) = input_area.suggestions_snapshot();
    model.input.completion.visible = show_suggestions;
    model.input.completion.selected_index = selected_suggestion;
    model.input.completion.items = suggestions
        .iter()
        .map(completion_item_from_suggestion)
        .collect();
}

pub(crate) fn apply_input_changes_to_legacy(
    input_area: &mut InputArea,
    _status_bar: &mut StatusBar,
    changes: &[InputChange],
) -> Option<String> {
    let mut submission = None;
    for change in changes {
        match change {
            InputChange::TextChanged { text, .. } | InputChange::HistorySelected { text } => {
                input_area.set_text(text);
            }
            InputChange::CompletionChanged { visible, items, .. } => {
                if *visible {
                    input_area.set_suggestions(
                        items.iter().map(suggestion_from_completion_item).collect(),
                    );
                } else {
                    input_area.clear_suggestions();
                }
            }
            InputChange::Submitted {
                submission: submitted,
            } => {
                submission = Some(submitted.text.clone());
                input_area.clear();
            }
            InputChange::Cleared => {
                input_area.clear();
            }
            InputChange::AttachmentChanged { count } => {
                input_area.set_pending_images(*count);
            }
            InputChange::ModeChanged { .. } | InputChange::CursorMoved { .. } => {}
        }
    }
    submission
}

#[cfg(test)]
pub(crate) fn submit_input_model(model: &mut crate::tui::model::root::TuiModel) -> Option<String> {
    let changes = model.input.apply(InputIntent::Submit);
    changes.into_iter().find_map(|change| match change {
        InputChange::Submitted { submission } => Some(submission.text),
        _ => None,
    })
}

fn completion_item_from_suggestion(suggestion: &Suggestion) -> CompletionItem {
    CompletionItem::new(&suggestion.display_text, &suggestion.display_text)
}

fn suggestion_from_completion_item(item: &CompletionItem) -> Suggestion {
    Suggestion {
        _id: item.label.clone(),
        display_text: item.label.clone(),
        _description: None,
        suggestion_type: crate::tui::completion::SuggestionType::Command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::root::TuiModel;

    #[test]
    fn test_mirror_input_area_to_model_copies_text() {
        let mut model = TuiModel::default();
        let mut input_area = InputArea::new();
        input_area.set_text("hello");

        mirror_input_area_to_model(&mut model, &input_area);

        assert_eq!(model.input.document.buffer, "hello");
    }

    #[test]
    fn test_submit_input_model_returns_and_clears() {
        let mut model = TuiModel::default();
        model
            .input
            .apply(InputIntent::InsertText("run".to_string()));

        let submitted = submit_input_model(&mut model);

        assert_eq!(submitted.as_deref(), Some("run"));
        assert!(model.input.document.buffer.is_empty());
    }

    #[test]
    fn test_apply_input_changes_to_legacy_applies_text_change() {
        let mut input_area = InputArea::new();
        let mut status_bar = StatusBar::new();
        let changes = vec![InputChange::TextChanged {
            text: "abc".to_string(),
            cursor: 3,
        }];

        apply_input_changes_to_legacy(&mut input_area, &mut status_bar, &changes);

        assert_eq!(input_area.get_text(), "abc");
    }
}
