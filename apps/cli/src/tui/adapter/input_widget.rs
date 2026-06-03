use crate::tui::model::input::change::InputChange;
use crate::tui::{InputArea, StatusBar};

pub(crate) fn apply_input_changes_to_widget(
    input_area: &mut InputArea,
    _status_bar: &mut StatusBar,
    changes: &[InputChange],
) -> Option<String> {
    let mut submission = None;
    for change in changes {
        match change {
            InputChange::TextChanged { .. }
            | InputChange::HistorySelected { .. }
            | InputChange::CursorMoved { .. }
            | InputChange::CompletionChanged { .. }
            | InputChange::AttachmentChanged { .. } => {}
            InputChange::Submitted {
                submission: submitted,
            } => {
                submission = Some(submitted.text.clone());
                input_area.clear();
            }
            InputChange::Cleared => {
                input_area.clear();
            }
            InputChange::ModeChanged { .. } => {}
        }
    }
    submission
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_input_changes_to_widget_ignores_text_change_mirror() {
        let mut input_area = InputArea::new();
        let mut status_bar = StatusBar::new();
        let changes = vec![InputChange::TextChanged {
            text: "abc".to_string(),
            cursor: 3,
        }];

        let submitted = apply_input_changes_to_widget(&mut input_area, &mut status_bar, &changes);

        assert_eq!(submitted, None);
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
    }
}
