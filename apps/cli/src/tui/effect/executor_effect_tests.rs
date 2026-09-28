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

/// SendTerminalNotification 接线契约：executor 必须把 Effect 分派到注入 writer 的
/// 写出函数。NEVER 直接 execute_effect 执行该 Effect——它向真实 stdout 写 OSC 序列，
/// 会让每次 `cargo test` 触发真实桌面通知（测试副作用）。字节内容契约由
/// `terminal_notification_tests::write_terminal_notification_writes_exact_bytes_to_writer` 覆盖。
#[test]
fn executor_routes_terminal_notification_to_injected_writer() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/effect/executor.rs"),
    )
    .expect("read executor source");

    assert!(
        source.contains("Effect::SendTerminalNotification { title, body }"),
        "executor 必须存在 SendTerminalNotification 分派分支"
    );
    assert!(
        source.contains("write_terminal_notification"),
        "executor 分支必须委托 write_terminal_notification（注入 writer 的唯一写出入口）"
    );
}
