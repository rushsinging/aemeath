//! #59 S5：/save、/memory、/paste 改 push Effect 后的行为测试。
//! 验证副作用经 Effect/executor 执行、结果经 UiEvent 回灌，行为不变。

use super::event::UiEvent;
use super::slash_tests::app_with_blocking_reflection_client;
use crate::tui::effect::effect::Effect;

/// A2：/save 经 `Effect::SaveSession { notify: true }` 保存成功后回灌
/// `UiEvent::SessionSaved`，update 据此推送 `[session saved: id]` 反馈行。
#[tokio::test]
#[ignore = "#567: SaveSession 正在迁移到事件流，Effect 路径将被删除"]
async fn test_save_session_effect_notify_emits_session_saved() {
    let (mut app, _started_rx, _finish_tx) = app_with_blocking_reflection_client();
    app.chat.messages.push(sdk::ChatMessage::user_text("hello"));

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    app.execute_effect(Effect::SaveSession { notify: true }, &tx)
        .await;

    // #497：spawn_guarded 后台执行，需 yield 让后台任务完成。
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let event = rx.try_recv().expect("/save 应回灌 SessionSaved 事件");
    assert!(
        matches!(event, UiEvent::SessionSaved { ref id } if id == "test-session"),
        "应回灌带 session id 的 SessionSaved 事件，实际: {event:?}"
    );
}

/// A2 边界：后台自动保存（notify=false）在无消息时静默，不回灌任何反馈事件。
#[tokio::test]
async fn test_save_session_effect_silent_when_not_notify_and_empty() {
    let (mut app, _started_rx, _finish_tx) = app_with_blocking_reflection_client();
    // 无消息 + notify=false（MessagesSync 自动保存路径）。
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    app.execute_effect(Effect::SaveSession { notify: false }, &tx)
        .await;

    assert!(
        rx.try_recv().is_err(),
        "后台自动保存空会话不应回灌任何反馈事件"
    );
}

/// A2 错误路径：无 agent client 时 /save 回灌 SlashCommandFailed 反馈。
#[tokio::test]
async fn test_save_session_effect_no_client_emits_failure() {
    let mut app = super::App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    app.chat.messages.push(sdk::ChatMessage::user_text("hi"));

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    app.execute_effect(Effect::SaveSession { notify: true }, &tx)
        .await;

    // #497：spawn_guarded 后台执行，需 yield 让后台任务完成。
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let event = rx.try_recv().expect("/save 无 client 应回灌失败事件");
    assert!(
        matches!(event, UiEvent::SlashCommandFailed { ref message } if message.contains("Failed to save session")),
        "应回灌保存失败反馈，实际: {event:?}"
    );
}

/// A3 边界：无 agent client 时 /memory 不回灌任何事件（静默）。
#[tokio::test]
async fn test_fetch_memory_list_effect_no_client_is_silent() {
    let mut app = super::App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    app.execute_effect(Effect::FetchMemoryList, &tx).await;

    assert!(rx.try_recv().is_err(), "无 client 时 /memory 不应回灌事件");
}

/// A4：/paste 经既有 `Effect::ReadClipboardImage` 执行。mock 剪贴板读取失败时
/// executor 仅记录日志、不 panic、不添加待发送图片。
#[tokio::test]
async fn test_read_clipboard_image_effect_handles_failure_gracefully() {
    let (mut app, _started_rx, _finish_tx) = app_with_blocking_reflection_client();

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.execute_effect(Effect::ReadClipboardImage, &tx).await;

    assert_eq!(
        app.model.input.document.image_spans.len(),
        0,
        "剪贴板读取失败时不应添加待发送图片"
    );
}

/// A4：/paste 分发路由正确进入 ReadClipboardImage Effect 路径（mock 失败时
/// 不添加图片、不 panic），验证 dispatch 已去除 block_on。
#[tokio::test]
async fn test_paste_dispatch_routes_to_read_clipboard_effect() {
    let (mut app, _started_rx, _finish_tx) = app_with_blocking_reflection_client();

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let prompt = app
        .handle_slash_command_with_events("/paste", Some(tx))
        .await;

    assert!(prompt.is_none(), "/paste 不应返回 LLM prompt");
    assert_eq!(
        app.model.input.document.image_spans.len(),
        0,
        "mock 剪贴板失败时 /paste 不应添加图片"
    );
}
