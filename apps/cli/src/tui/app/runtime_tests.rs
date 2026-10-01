use super::App;
use sdk::CharIdx;
use std::path::PathBuf;

fn test_app() -> App {
    App::new(
        "test-session".to_string(),
        PathBuf::from("/tmp"),
        "test-model".to_string(),
    )
}

#[test]
fn reset_runtime_state_clears_view_state_selection_truth() {
    use crate::tui::render::status::StatusBarRow;
    let mut app = test_app();
    // 在 view_state（三区选区真相）中建立选区。
    app.view_state.output.begin_selection(0, CharIdx::new(1));
    app.view_state.output.update_selection(0, CharIdx::new(5));
    assert!(app.view_state.output.selection_range().is_some());
    app.view_state
        .status_sel
        .begin_selection(StatusBarRow::Runtime, 2, 80);
    app.view_state.status_sel.update_selection(6);
    assert!(app.view_state.status_sel.selection_range().is_some());
    app.view_state.input_sel.begin_selection((0, 2));
    app.view_state.input_sel.update_selection((0, 6));
    assert!(app.view_state.input_sel.normalized_selection().is_some());

    app.reset_runtime_state();

    // 三区真相被清空。
    assert_eq!(app.view_state.output.selection_range(), None);
    assert!(!app.view_state.output.is_selecting());
    assert_eq!(app.view_state.status_sel.selection_range(), None);
    assert!(!app.view_state.status_sel.is_selecting());
    assert_eq!(app.view_state.input_sel.normalized_selection(), None);
    assert!(!app.view_state.input_sel.is_selecting());
}
