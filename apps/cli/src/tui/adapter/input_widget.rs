use crate::tui::model::input::change::InputChange;
use crate::tui::model::input::completion::Suggestion;
use crate::tui::model::input::completion_item::CompletionItem;
use crate::tui::{InputArea, StatusBar};

pub(crate) fn apply_input_changes_to_widget(
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

pub(crate) fn completion_item_from_suggestion(suggestion: &Suggestion) -> CompletionItem {
    CompletionItem::new(&suggestion.display_text, &suggestion.display_text)
}

fn suggestion_from_completion_item(item: &CompletionItem) -> Suggestion {
    Suggestion {
        _id: item.label.clone(),
        display_text: item.label.clone(),
        _description: None,
        suggestion_type: crate::tui::model::input::completion::SuggestionType::Command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_input_changes_to_widget_applies_text_change() {
        let mut input_area = InputArea::new();
        let mut status_bar = StatusBar::new();
        let changes = vec![InputChange::TextChanged {
            text: "abc".to_string(),
            cursor: 3,
        }];

        apply_input_changes_to_widget(&mut input_area, &mut status_bar, &changes);

        assert_eq!(input_area.get_text(), "abc");
    }

    #[test]
    fn test_completion_item_from_suggestion_uses_display_text() {
        let suggestion = Suggestion {
            _id: "cmd".to_string(),
            display_text: "/help".to_string(),
            _description: None,
            suggestion_type: crate::tui::model::input::completion::SuggestionType::Command,
        };

        let item = completion_item_from_suggestion(&suggestion);

        assert_eq!(item.label, "/help");
        assert_eq!(item.replacement, "/help");
    }

    #[test]
    fn test_apply_input_changes_to_widget_returns_submission() {
        let mut input_area = InputArea::new();
        let mut status_bar = StatusBar::new();
        let changes = vec![InputChange::Submitted {
            submission: crate::tui::model::input::submission::InputSubmission {
                text: "run".to_string(),
                attachments: Vec::new(),
            },
        }];

        let submitted = apply_input_changes_to_widget(&mut input_area, &mut status_bar, &changes);

        assert_eq!(submitted.as_deref(), Some("run"));
        assert!(input_area.is_empty());
    }
}
