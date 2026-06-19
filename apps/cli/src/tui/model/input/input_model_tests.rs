use super::change::InputChange;
use super::intent::InputIntent;
use super::model::InputModel;
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
    model.apply(InputIntent::InsertPastedText("a\nb\nc\nd".to_string()));

    assert_eq!(model.document.buffer, "[Copied 4 lines]");

    let changes = model.apply(InputIntent::Submit);
    let submission = changes
        .iter()
        .find_map(|change| match change {
            InputChange::Submitted { submission } => Some(submission),
            _ => None,
        })
        .expect("应产生提交变更");
    assert_eq!(submission.text, "a\nb\nc\nd");
    assert_eq!(submission.display_text, "[Copied 4 lines]");
}

#[test]
fn test_input_model_does_not_collapse_two_line_paste() {
    let mut model = InputModel::default();
    model.apply(InputIntent::InsertPastedText("a\nb".to_string()));

    assert_eq!(model.document.buffer, "a\nb");
}

#[test]
fn test_input_model_does_not_collapse_three_line_paste() {
    let mut model = InputModel::default();
    model.apply(InputIntent::InsertPastedText("a\nb\nc".to_string()));

    assert_eq!(model.document.buffer, "a\nb\nc");
}

#[test]
fn test_input_model_backspace_deletes_copied_text_as_atomic_block() {
    let mut model = InputModel::default();
    model.apply(InputIntent::InsertPastedText("a\nb\nc\nd".to_string()));

    model.apply(InputIntent::DeleteBackward);

    assert_eq!(model.document.buffer, "");
    assert_eq!(model.document.cursor, 0);
    assert_eq!(model.document.expand_copied_text(), "");
}

#[test]
fn test_input_model_backspace_inside_copied_text_deletes_atomic_block() {
    let mut model = InputModel::default();
    model.apply(InputIntent::InsertPastedText("a\nb\nc\nd".to_string()));
    model.apply(InputIntent::MoveCursor(5));

    model.apply(InputIntent::DeleteBackward);

    assert_eq!(model.document.buffer, "");
    assert_eq!(model.document.cursor, 0);
    assert_eq!(model.document.expand_copied_text(), "");
}

#[test]
fn test_input_model_ctrl_backspace_deletes_copied_text_as_atomic_block() {
    let mut model = InputModel::default();
    model.apply(InputIntent::InsertPastedText("a\nb\nc\nd".to_string()));

    model.apply(InputIntent::DeleteWordBeforeCursor);

    assert_eq!(model.document.buffer, "");
    assert_eq!(model.document.cursor, 0);
    assert_eq!(model.document.expand_copied_text(), "");
}

#[test]
fn test_input_model_copied_text_counter_increments_per_long_paste() {
    let mut model = InputModel::default();
    model.apply(InputIntent::InsertPastedText("a\nb\nc\nd".to_string()));
    model.apply(InputIntent::InsertText(" ".to_string()));
    model.apply(InputIntent::InsertPastedText("d\ne\nf\ng".to_string()));

    assert_eq!(model.document.buffer, "[Copied 4 lines] [Copied 4 lines]");
    assert_eq!(model.document.expand_copied_text(), "a\nb\nc\nd d\ne\nf\ng");
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
