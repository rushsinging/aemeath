use super::attachment::InputAttachment;
use super::change::InputChange;
use super::completion::InputCompletion;
use super::document::InputDocument;
use super::history::InputHistory;
use super::intent::InputIntent;
use super::mode::InputMode;
use super::submission::InputSubmission;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputModel {
    pub document: InputDocument,
    pub history: InputHistory,
    pub completion: InputCompletion,
    pub attachments: Vec<InputAttachment>,
    pub mode: InputMode,
}

impl InputModel {
    pub fn apply(&mut self, intent: InputIntent) -> Vec<InputChange> {
        match intent {
            InputIntent::InsertChar(ch) => self.insert_text(ch.to_string()),
            InputIntent::InsertText(text) => self.insert_text(text),
            InputIntent::ReplaceText(text) => self.replace_text(text),
            InputIntent::MoveCursor(cursor) => {
                self.document.move_cursor(cursor);
                vec![InputChange::CursorMoved {
                    cursor: self.document.cursor,
                }]
            }
            InputIntent::MoveCursorLeft => {
                self.document.move_left();
                vec![InputChange::CursorMoved {
                    cursor: self.document.cursor,
                }]
            }
            InputIntent::MoveCursorRight => {
                self.document.move_right();
                vec![InputChange::CursorMoved {
                    cursor: self.document.cursor,
                }]
            }
            InputIntent::MoveCursorHome => {
                self.document.move_home();
                vec![InputChange::CursorMoved {
                    cursor: self.document.cursor,
                }]
            }
            InputIntent::MoveCursorEnd => {
                self.document.move_end();
                vec![InputChange::CursorMoved {
                    cursor: self.document.cursor,
                }]
            }
            InputIntent::InsertNewline => self.insert_text("\n".to_string()),
            InputIntent::DeleteBackward => {
                self.completion.clear();
                self.document.delete_backward();
                self.text_changed()
            }
            InputIntent::DeleteWordBeforeCursor => {
                self.completion.clear();
                self.document.delete_word_before_cursor();
                self.text_changed()
            }
            InputIntent::DeleteForward => {
                self.completion.clear();
                self.document.delete_forward();
                self.text_changed()
            }
            InputIntent::MoveHistoryPrevious => self.history_previous(),
            InputIntent::MoveHistoryNext => self.history_next(),
            InputIntent::SetCompletions { query, items } => {
                self.completion.set_items(items, query);
                self.mode = if self.completion.visible {
                    InputMode::Completion
                } else {
                    InputMode::Normal
                };
                vec![
                    self.completion_changed(),
                    InputChange::ModeChanged { mode: self.mode },
                ]
            }
            InputIntent::SelectCompletionNext => {
                self.completion.select_next();
                vec![self.completion_changed()]
            }
            InputIntent::SelectCompletionPrevious => {
                self.completion.select_previous();
                vec![self.completion_changed()]
            }
            InputIntent::AcceptCompletion => self.accept_completion(),
            InputIntent::AcceptCompletionValue(replacement) => {
                self.accept_completion_value(replacement)
            }
            InputIntent::SetAttachmentCount(count) => {
                vec![InputChange::AttachmentChanged { count }]
            }
            InputIntent::SetMode(mode) => {
                self.mode = mode;
                vec![InputChange::ModeChanged { mode }]
            }
            InputIntent::Submit => self.submit(),
            InputIntent::Clear => {
                self.document.clear();
                self.completion.clear();
                vec![InputChange::Cleared]
            }
        }
    }

    fn insert_text(&mut self, text: String) -> Vec<InputChange> {
        self.completion.clear();
        self.history.selected_index = None;
        self.document.insert_text(&text);
        self.text_changed()
    }

    fn replace_text(&mut self, text: String) -> Vec<InputChange> {
        self.completion.clear();
        self.history.selected_index = None;
        self.document.replace_text(text);
        self.text_changed()
    }

    fn text_changed(&self) -> Vec<InputChange> {
        vec![InputChange::TextChanged {
            text: self.document.buffer.clone(),
            cursor: self.document.cursor,
        }]
    }

    fn completion_changed(&self) -> InputChange {
        InputChange::CompletionChanged {
            visible: self.completion.visible,
            selected_index: self.completion.selected_index,
            items: self.completion.items.clone(),
        }
    }

    fn accept_completion(&mut self) -> Vec<InputChange> {
        let Some(replacement) = self
            .completion
            .selected_item()
            .map(|item| item.replacement.clone())
        else {
            return vec![self.completion_changed()];
        };
        self.accept_completion_value(replacement)
    }

    fn accept_completion_value(&mut self, replacement: String) -> Vec<InputChange> {
        self.document.replace_text(replacement);
        self.completion.clear();
        self.mode = InputMode::Normal;
        vec![
            InputChange::TextChanged {
                text: self.document.buffer.clone(),
                cursor: self.document.cursor,
            },
            self.completion_changed(),
            InputChange::ModeChanged { mode: self.mode },
        ]
    }

    fn history_previous(&mut self) -> Vec<InputChange> {
        if self.history.entries.is_empty() {
            return Vec::new();
        }
        if self.history.selected_index.is_none() {
            self.history.saved_input = self.document.buffer.clone();
            self.history.selected_index = Some(self.history.entries.len() - 1);
        } else if let Some(index) = self.history.selected_index {
            self.history.selected_index = Some(index.saturating_sub(1));
        }
        self.apply_history_selection()
    }

    fn history_next(&mut self) -> Vec<InputChange> {
        let Some(index) = self.history.selected_index else {
            return Vec::new();
        };
        if index + 1 >= self.history.entries.len() {
            self.history.selected_index = None;
            let saved = self.history.saved_input.clone();
            self.document.clear();
            self.document.insert_text(&saved);
        } else {
            self.history.selected_index = Some(index + 1);
        }
        self.apply_history_selection()
    }

    fn apply_history_selection(&mut self) -> Vec<InputChange> {
        if let Some(index) = self.history.selected_index {
            if let Some(text) = self.history.entries.get(index).cloned() {
                self.document.clear();
                self.document.insert_text(&text);
            }
        }
        vec![
            InputChange::HistorySelected {
                text: self.document.buffer.clone(),
                cursor: self.document.cursor,
            },
            InputChange::TextChanged {
                text: self.document.buffer.clone(),
                cursor: self.document.cursor,
            },
        ]
    }

    fn submit(&mut self) -> Vec<InputChange> {
        let submission = InputSubmission {
            text: self.document.buffer.clone(),
            attachments: self.attachments.clone(),
        };
        self.history.entries.push(submission.text.clone());
        self.history.selected_index = None;
        self.history.saved_input.clear();
        self.attachments.clear();
        self.completion.clear();
        self.mode = InputMode::Normal;
        self.document.clear();
        vec![
            InputChange::Submitted { submission },
            InputChange::AttachmentChanged { count: 0 },
            InputChange::ModeChanged { mode: self.mode },
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
