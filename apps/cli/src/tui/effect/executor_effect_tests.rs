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

/// FetchMemoryList：loop 未运行（tx 缺失）时静默，不回灌任何 UiEvent。
#[tokio::test]
async fn fetch_memory_list_effect_is_silent_without_input_channel() {
    let mut app = App::new(
        "s".to_string(),
        std::path::PathBuf::from("/tmp"),
        "m".to_string(),
    );

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    app.execute_effect(Effect::FetchMemoryList, &tx).await;

    assert!(
        rx.try_recv().is_err(),
        "无输入通道时 FetchMemoryList 不应回灌事件"
    );
}
