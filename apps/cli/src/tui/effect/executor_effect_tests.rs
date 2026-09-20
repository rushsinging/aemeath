use super::*;

#[test]
fn effect_runtime_ignores_noop_effect() {
    let app = App::new(
        "s".to_string(),
        std::path::PathBuf::from("/tmp"),
        "m".to_string(),
    );
    assert!(!app.layout.should_exit);
}

#[test]
fn effect_runtime_quit_effect_sets_exit_flag() {
    let mut app = App::new(
        "s".to_string(),
        std::path::PathBuf::from("/tmp"),
        "m".to_string(),
    );
    app.layout.request_exit();
    assert!(app.layout.should_exit);
}

#[test]
fn effect_runtime_accepts_pending_image() {
    let mut app = App::new(
        "s".to_string(),
        std::path::PathBuf::from("/tmp"),
        "m".to_string(),
    );
    // accept_pending_clipboard_image 已移除（spawn_guarded 化），
    // 图片经 UiEvent::ClipboardImage → InsertImage intent 注入。
    app.handle_input_intent(crate::tui::model::input::intent::InputIntent::InsertImage(
        sdk::ClipboardImageView {
            base64: "abc".to_string(),
            media_type: "image/png".to_string(),
            final_size: 3,
            display_path: None,
            width: None,
            height: None,
        },
    ));
    assert_eq!(app.model.input.document.image_spans.len(), 1);
}
