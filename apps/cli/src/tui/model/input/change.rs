use super::completion_item::CompletionItem;
use super::mode::InputMode;
use super::submission::InputSubmission;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputChange {
    TextChanged {
        text: String,
        cursor: usize,
    },
    CursorMoved {
        cursor: usize,
    },
    CompletionChanged {
        visible: bool,
        selected_index: Option<usize>,
        items: Vec<CompletionItem>,
    },
    HistorySelected {
        text: String,
        cursor: usize,
    },
    ModeChanged {
        mode: InputMode,
    },
    Submitted {
        submission: InputSubmission,
    },
    Cleared,
}

pub fn submitted_text_from_changes(changes: &[InputChange]) -> Option<String> {
    changes.iter().find_map(|change| match change {
        InputChange::Submitted { submission } => Some(submission.text.clone()),
        InputChange::TextChanged { .. }
        | InputChange::CursorMoved { .. }
        | InputChange::CompletionChanged { .. }
        | InputChange::HistorySelected { .. }
        | InputChange::ModeChanged { .. }
        | InputChange::Cleared => None,
    })
}

pub fn submitted_display_text_from_changes(changes: &[InputChange]) -> Option<String> {
    changes.iter().find_map(|change| match change {
        InputChange::Submitted { submission } => Some(submission.display_text.clone()),
        InputChange::TextChanged { .. }
        | InputChange::CursorMoved { .. }
        | InputChange::CompletionChanged { .. }
        | InputChange::HistorySelected { .. }
        | InputChange::ModeChanged { .. }
        | InputChange::Cleared => None,
    })
}

pub fn submitted_submission_from_changes(changes: &[InputChange]) -> Option<InputSubmission> {
    changes.iter().find_map(|change| match change {
        InputChange::Submitted { submission } => Some(submission.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submitted_text_from_changes_returns_submission_text() {
        let changes = vec![InputChange::Submitted {
            submission: InputSubmission {
                text: "run".to_string(),
                display_text: "run".to_string(),
                images: Vec::new(),
            },
        }];

        let submitted = submitted_text_from_changes(&changes);

        assert_eq!(submitted.as_deref(), Some("run"));
    }

    #[test]
    fn test_submitted_text_from_changes_ignores_non_submission_changes() {
        let changes = vec![
            InputChange::TextChanged {
                text: "abc".to_string(),
                cursor: 3,
            },
            InputChange::CursorMoved { cursor: 1 },
            InputChange::Cleared,
        ];

        let submitted = submitted_text_from_changes(&changes);

        assert_eq!(submitted, None);
    }

    #[test]
    fn test_submitted_text_from_changes_returns_first_submission() {
        let changes = vec![
            InputChange::Submitted {
                submission: InputSubmission {
                    text: "first".to_string(),
                    display_text: "first".to_string(),
                    images: Vec::new(),
                },
            },
            InputChange::Submitted {
                submission: InputSubmission {
                    text: "second".to_string(),
                    display_text: "second".to_string(),
                    images: Vec::new(),
                },
            },
        ];

        let submitted = submitted_text_from_changes(&changes);

        assert_eq!(submitted.as_deref(), Some("first"));
    }
}
