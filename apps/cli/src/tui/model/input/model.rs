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
        let submission = InputSubmission {
            text: self.document.expand_copied_text(),
            display_text: self.document.display_text(),
            attachments: self.attachments.clone(),
        };
        self.history.entries.push(submission.display_text.clone());
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

fn find_token_end(input: &str, cursor_offset: usize) -> usize {
    let remaining = input.get(cursor_offset..).unwrap_or("");
    if let Some(space_pos) = remaining.find(' ') {
        cursor_offset + space_pos
    } else {
        input.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::input::completion::SuggestionType;
    use crate::tui::model::input::completion_item::CompletionItem;

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

    #[test]
    fn test_input_model_replace_history_allows_previous_navigation() {
        let mut model = InputModel::default();
        model.apply(InputIntent::ReplaceHistory(vec![
            "first".to_string(),
            "second".to_string(),
        ]));

        let changes = model.apply(InputIntent::MoveHistoryPrevious);

        assert_eq!(model.document.buffer, "second");
        assert_eq!(model.history.selected_index, Some(1));
        assert!(changes.iter().any(|change| matches!(
            change,
            InputChange::HistorySelected { text, cursor } if text == "second" && *cursor == 6
        )));
    }

    #[test]
    fn test_input_model_history_next_restores_saved_draft() {
        let mut model = InputModel::default();
        model.apply(InputIntent::ReplaceHistory(vec!["past".to_string()]));
        model.apply(InputIntent::InsertText("draft".to_string()));
        model.apply(InputIntent::MoveHistoryPrevious);

        model.apply(InputIntent::MoveHistoryNext);

        assert_eq!(model.document.buffer, "draft");
        assert_eq!(model.history.selected_index, None);
    }

    #[test]
    fn test_input_model_replace_history_clears_active_selection() {
        let mut model = InputModel::default();
        model.apply(InputIntent::ReplaceHistory(vec!["old".to_string()]));
        model.apply(InputIntent::MoveHistoryPrevious);

        model.apply(InputIntent::ReplaceHistory(vec!["new".to_string()]));

        assert_eq!(model.history.entries, vec!["new".to_string()]);
        assert_eq!(model.history.selected_index, None);
        assert_eq!(model.history.saved_input, "");
    }

    #[test]
    fn test_input_model_collapses_long_pasted_text_and_submits_original() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertPastedText("a\nb\nc".to_string()));

        assert_eq!(model.document.buffer, "[Copied Text 1]");

        let changes = model.apply(InputIntent::Submit);
        let submission = changes
            .iter()
            .find_map(|change| match change {
                InputChange::Submitted { submission } => Some(submission),
                _ => None,
            })
            .expect("应产生提交变更");
        assert_eq!(submission.text, "a\nb\nc");
        assert_eq!(submission.display_text, "[Copied Text 1]");
    }

    #[test]
    fn test_input_model_does_not_collapse_two_line_paste() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertPastedText("a\nb".to_string()));

        assert_eq!(model.document.buffer, "a\nb");
    }

    #[test]
    fn test_input_model_backspace_deletes_copied_text_as_atomic_block() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertPastedText("a\nb\nc".to_string()));

        model.apply(InputIntent::DeleteBackward);

        assert_eq!(model.document.buffer, "");
        assert_eq!(model.document.cursor, 0);
        assert_eq!(model.document.expand_copied_text(), "");
    }

    #[test]
    fn test_input_model_backspace_inside_copied_text_deletes_atomic_block() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertPastedText("a\nb\nc".to_string()));
        model.apply(InputIntent::MoveCursor(5));

        model.apply(InputIntent::DeleteBackward);

        assert_eq!(model.document.buffer, "");
        assert_eq!(model.document.cursor, 0);
        assert_eq!(model.document.expand_copied_text(), "");
    }

    #[test]
    fn test_input_model_ctrl_backspace_deletes_copied_text_as_atomic_block() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertPastedText("a\nb\nc".to_string()));

        model.apply(InputIntent::DeleteWordBeforeCursor);

        assert_eq!(model.document.buffer, "");
        assert_eq!(model.document.cursor, 0);
        assert_eq!(model.document.expand_copied_text(), "");
    }

    #[test]
    fn test_input_model_copied_text_counter_increments_per_long_paste() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertPastedText("a\nb\nc".to_string()));
        model.apply(InputIntent::InsertText(" ".to_string()));
        model.apply(InputIntent::InsertPastedText("d\ne\nf".to_string()));

        assert_eq!(model.document.buffer, "[Copied Text 1] [Copied Text 2]");
        assert_eq!(model.document.expand_copied_text(), "a\nb\nc d\ne\nf");
    }

    #[test]
    fn test_accept_completion_replaces_slash_token() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("/he now".to_string()));
        model.apply(InputIntent::MoveCursor(3));
        model.apply(InputIntent::SetCompletions {
            query: "/he now".to_string(),
            items: vec![CompletionItem::new("/help", "/help")],
        });

        model.apply(InputIntent::AcceptCompletion);

        assert_eq!(model.document.buffer, "/help now");
        assert_eq!(model.document.cursor, 9);
        assert!(!model.completion.visible);
    }

    #[test]
    fn test_accept_completion_replaces_at_token_with_prefix() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("read @src/main tail".to_string()));
        model.apply(InputIntent::MoveCursor(14));
        model.apply(InputIntent::SetCompletions {
            query: "read @src/main tail".to_string(),
            items: vec![CompletionItem::with_type(
                "src/main.rs",
                "src/main.rs",
                SuggestionType::File,
            )],
        });

        model.apply(InputIntent::AcceptCompletion);

        assert_eq!(model.document.buffer, "read @src/main.rs tail");
        assert_eq!(model.document.cursor, 22);
    }

    #[test]
    fn test_accept_completion_rewrites_session_resume_argument() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("/resume old".to_string()));
        model.apply(InputIntent::SetCompletions {
            query: "/resume old".to_string(),
            items: vec![CompletionItem::with_type(
                "s-123 previous",
                "s-123 previous",
                SuggestionType::Session,
            )],
        });

        model.apply(InputIntent::AcceptCompletion);

        assert_eq!(model.document.buffer, "/resume s-123");
    }

    // Bug #99: MoveCursorUp/Down 在多行时移动光标，在边界时翻历史

    #[test]
    fn test_move_cursor_up_multiline_moves_cursor() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("line1\nline2".to_string()));
        // 光标在第二行末尾
        assert!(model.document.is_cursor_at_last_line());
        let changes = model.apply(InputIntent::MoveCursorUp);
        // 应移动光标到第一行，不是翻历史
        assert!(model.document.is_cursor_at_first_line());
        assert!(changes
            .iter()
            .any(|c| matches!(c, InputChange::CursorMoved { .. })));
    }

    #[test]
    fn test_move_cursor_up_at_first_line_triggers_history() {
        let mut model = InputModel::default();
        model.apply(InputIntent::ReplaceHistory(vec![
            "history_entry".to_string()
        ]));
        model.apply(InputIntent::InsertText("current".to_string()));
        // 光标在第一行（也是唯一一行），按 Up 应翻历史
        let changes = model.apply(InputIntent::MoveCursorUp);
        assert_eq!(model.document.buffer, "history_entry");
        assert!(changes
            .iter()
            .any(|c| matches!(c, InputChange::HistorySelected { .. })));
    }

    #[test]
    fn test_move_cursor_down_at_last_line_triggers_history() {
        let mut model = InputModel::default();
        model.apply(InputIntent::ReplaceHistory(vec!["past".to_string()]));
        model.apply(InputIntent::InsertText("draft".to_string()));
        // 先翻到历史
        model.apply(InputIntent::MoveCursorUp);
        assert_eq!(model.document.buffer, "past");
        // 在最后一行按 Down 应翻回
        let changes = model.apply(InputIntent::MoveCursorDown);
        assert_eq!(model.document.buffer, "draft");
        assert!(changes
            .iter()
            .any(|c| matches!(c, InputChange::HistorySelected { .. })));
    }

    #[test]
    fn test_move_cursor_down_multiline_moves_cursor() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("line1\nline2".to_string()));
        model.apply(InputIntent::MoveCursorHome); // 光标到第一行开头
        assert!(model.document.is_cursor_at_first_line());
        let changes = model.apply(InputIntent::MoveCursorDown);
        // 应移动光标到第二行
        assert!(model.document.is_cursor_at_last_line());
        assert!(changes
            .iter()
            .any(|c| matches!(c, InputChange::CursorMoved { .. })));
    }
}
