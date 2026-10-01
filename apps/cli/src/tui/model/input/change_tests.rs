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
