use super::attachment::InputAttachment;
use super::change::InputChange;
use super::completion::InputCompletion;
use super::document::InputDocument;
use super::history::InputHistory;
use super::intent::InputIntent;
use super::submission::InputSubmission;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputModel {
    pub document: InputDocument,
    pub history: InputHistory,
    pub completion: InputCompletion,
    pub attachments: Vec<InputAttachment>,
}

impl InputModel {
    pub fn apply(&mut self, intent: InputIntent) -> Vec<InputChange> {
        match intent {
            InputIntent::InsertText(text) => {
                self.document.insert_text(&text);
                vec![InputChange::TextChanged {
                    text: self.document.buffer.clone(),
                    cursor: self.document.cursor,
                }]
            }
            InputIntent::MoveCursor(cursor) => {
                self.document.move_cursor(cursor);
                vec![InputChange::CursorMoved {
                    cursor: self.document.cursor,
                }]
            }
            InputIntent::DeleteBackward => {
                self.document.delete_backward();
                vec![InputChange::TextChanged {
                    text: self.document.buffer.clone(),
                    cursor: self.document.cursor,
                }]
            }
            InputIntent::Submit => self.submit(),
            InputIntent::Clear => {
                self.document.clear();
                vec![InputChange::Cleared]
            }
        }
    }

    fn submit(&mut self) -> Vec<InputChange> {
        let submission = InputSubmission {
            text: self.document.buffer.clone(),
            attachments: self.attachments.clone(),
        };
        self.history.entries.push(submission.text.clone());
        self.document.clear();
        vec![
            InputChange::Submitted { submission },
            InputChange::Cleared,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_model_insert_text_emits_change() {
        let mut model = InputModel::default();
        let changes = model.apply(InputIntent::InsertText("hi".to_string()));
        assert!(matches!(
            changes.first(),
            Some(InputChange::TextChanged { text, cursor }) if text == "hi" && *cursor == 2
        ));
    }

    #[test]
    fn test_input_model_submit_returns_submission_and_clears() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("run".to_string()));
        let changes = model.apply(InputIntent::Submit);
        assert!(changes.iter().any(|change| matches!(
            change,
            InputChange::Submitted { submission } if submission.text == "run"
        )));
        assert_eq!(model.document.buffer, "");
    }

    #[test]
    fn test_input_model_delete_backward_on_empty_stays_empty() {
        let mut model = InputModel::default();
        let changes = model.apply(InputIntent::DeleteBackward);
        assert_eq!(model.document.buffer, "");
        assert!(matches!(
            changes.first(),
            Some(InputChange::TextChanged { text, cursor }) if text.is_empty() && *cursor == 0
        ));
    }
}
