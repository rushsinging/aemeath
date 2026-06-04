use crate::tui::model::input::change::InputChange;

pub(crate) fn submission_from_changes(changes: &[InputChange]) -> Option<String> {
    changes.iter().find_map(|change| match change {
        InputChange::Submitted { submission } => Some(submission.text.clone()),
        InputChange::TextChanged { .. }
        | InputChange::HistorySelected { .. }
        | InputChange::CursorMoved { .. }
        | InputChange::CompletionChanged { .. }
        | InputChange::AttachmentChanged { .. }
        | InputChange::Cleared
        | InputChange::ModeChanged { .. } => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submission_from_changes_ignores_text_change_mirror() {
        let changes = vec![InputChange::TextChanged {
            text: "abc".to_string(),
            cursor: 3,
        }];

        let submitted = submission_from_changes(&changes);

        assert_eq!(submitted, None);
    }

    #[test]
    fn test_submission_from_changes_returns_submission_text() {
        let changes = vec![InputChange::Submitted {
            submission: crate::tui::model::input::submission::InputSubmission {
                text: "run".to_string(),
                attachments: Vec::new(),
            },
        }];

        let submitted = submission_from_changes(&changes);

        assert_eq!(submitted.as_deref(), Some("run"));
    }

    #[test]
    fn test_submission_from_changes_returns_first_submission() {
        let changes = vec![
            InputChange::Submitted {
                submission: crate::tui::model::input::submission::InputSubmission {
                    text: "first".to_string(),
                    attachments: Vec::new(),
                },
            },
            InputChange::Submitted {
                submission: crate::tui::model::input::submission::InputSubmission {
                    text: "second".to_string(),
                    attachments: Vec::new(),
                },
            },
        ];

        let submitted = submission_from_changes(&changes);

        assert_eq!(submitted.as_deref(), Some("first"));
    }
}
