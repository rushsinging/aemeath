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
    pub mode: InputMode,
}

impl InputModel {
    pub fn apply(&mut self, intent: InputIntent) -> Vec<InputChange> {
        match intent {
            InputIntent::InsertChar(ch) => self.insert_text(ch.to_string()),
            InputIntent::InsertText(text) => self.insert_text(text),
            InputIntent::InsertPastedText(text) => self.insert_pasted_text(text),
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
            InputIntent::MoveCursorUp => {
                if self.document.is_cursor_at_first_line() {
                    self.history_previous()
                } else {
                    self.document.move_up();
                    vec![InputChange::CursorMoved {
                        cursor: self.document.cursor,
                    }]
                }
            }
            InputIntent::MoveCursorDown => {
                if self.document.is_cursor_at_last_line() {
                    self.history_next()
                } else {
                    self.document.move_down();
                    vec![InputChange::CursorMoved {
                        cursor: self.document.cursor,
                    }]
                }
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
            InputIntent::ReplaceHistory(entries) => {
                self.history.entries = entries;
                self.history.selected_index = None;
                self.history.saved_input.clear();
                Vec::new()
            }
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
                self.accept_completion_replacement(replacement)
            }
            InputIntent::InsertImage(image) => {
                self.completion.clear();
                self.history.selected_index = None;
                self.document.insert_image(image);
                self.text_changed()
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

    fn insert_pasted_text(&mut self, text: String) -> Vec<InputChange> {
        self.completion.clear();
        self.history.selected_index = None;
        self.document.insert_pasted_text(&text);
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
        let Some(item) = self.completion.selected_item().cloned() else {
            return vec![self.completion_changed()];
        };
        self.accept_completion_item(item)
    }

    fn accept_completion_item(
        &mut self,
        item: super::completion_item::CompletionItem,
    ) -> Vec<InputChange> {
        use super::completion::extract_completion_token;
        use super::completion::TriggerType;

        let current = self.document.buffer.clone();
        let cursor_offset = self.document.cursor;
        let replacement = item.replacement;
        let new_text = match item.suggestion_type {
            super::completion::SuggestionType::Session => {
                let id = replacement.split_whitespace().next().unwrap_or("");
                if let Some(space_pos) = current.find(' ') {
                    let prefix = current.get(..=space_pos).unwrap_or("");
                    format!("{}{}", prefix, id)
                } else {
                    format!("/resume {}", id)
                }
            }
            _ => {
                if let Some((_token, start_pos, trigger_type)) =
                    extract_completion_token(&current, cursor_offset)
                {
                    let before = current.get(..start_pos).unwrap_or("");
                    let after_end = find_token_end(&current, cursor_offset);
                    let after = current.get(after_end..).unwrap_or("");
                    match trigger_type {
                        TriggerType::AtSymbol => format!("{}@{}{}", before, replacement, after),
                        TriggerType::SlashCommand
                        | TriggerType::ModelArg
                        | TriggerType::ModelSubCommand
                        | TriggerType::ResumeArg => {
                            format!("{}{}{}", before, replacement, after)
                        }
                    }
                } else {
                    replacement
                }
            }
        };
        self.accept_completion_replacement(new_text)
    }

    fn accept_completion_replacement(&mut self, replacement: String) -> Vec<InputChange> {
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
        let text = self.document.submit_text();
        let display_text = self.document.display_text();
        let images = self.document.drain_images();
        let submission = InputSubmission {
            text,
            display_text,
            images,
        };
        self.history.entries.push(submission.display_text.clone());
        self.history.selected_index = None;
        self.history.saved_input.clear();
        self.completion.clear();
        self.mode = InputMode::Normal;
        self.document.clear();
        vec![
            InputChange::Submitted { submission },
            InputChange::ModeChanged { mode: self.mode },
            InputChange::Cleared,
        ]
    }
}

fn find_token_end(input: &str, cursor_offset: usize) -> usize {
    let remaining = input.get(cursor_offset..).unwrap_or("");
    if let Some(space_pos) = remaining.find(' ') {
        cursor_offset + space_pos
    } else {
        input.len()
    }
}
