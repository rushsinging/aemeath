use crate::tui::app::App;
use crate::tui::effect::effect::Effect;
use std::path::PathBuf;

fn make_app() -> App {
    App::new("s".to_string(), PathBuf::from("/tmp"), "m".to_string())
}

#[test]
fn test_copy_to_clipboard_returns_effect() {
    let app = make_app();
    let effect = app.copy_to_clipboard("hello");
    assert!(matches!(effect, Effect::CopyToClipboard { text } if text == "hello"));
}

#[test]
fn test_copy_selection_to_clipboard_some_returns_effect() {
    let app = make_app();
    let effect = app.copy_selection_to_clipboard(Some("sel".to_string()));
    assert!(matches!(
        effect,
        Some(Effect::CopyToClipboard { text }) if text == "sel"
    ));
}

#[test]
fn test_copy_selection_to_clipboard_none_returns_none() {
    let app = make_app();
    assert!(app.copy_selection_to_clipboard(None).is_none());
}

#[test]
fn test_apply_current_suggestion_accepts_model_completion() {
    let mut app = make_app();
    app.handle_input_intent(crate::tui::model::input::intent::InputIntent::InsertText(
        "/he now".to_string(),
    ));
    for _ in 0..4 {
        app.handle_input_intent(crate::tui::model::input::intent::InputIntent::MoveCursorLeft);
    }
    app.handle_input_intent(
        crate::tui::model::input::intent::InputIntent::SetCompletions {
            query: "/he now".to_string(),
            items: vec![
                crate::tui::model::input::completion_item::CompletionItem::new("/help", "/help"),
            ],
        },
    );

    app.apply_current_suggestion();

    assert_eq!(app.model.input.document.buffer, "/help now");
}
