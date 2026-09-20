use super::*;
use crate::tui::app::UiEvent;
use crate::tui::effect::session::processing::SpawnContextRefs;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::mpsc;

fn test_app() -> App {
    App::new(
        "image-effect-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "image-effect-model".to_string(),
    )
}

async fn recv_event(ui_rx: &mut mpsc::Receiver<UiEvent>) -> UiEvent {
    tokio::time::timeout(Duration::from_secs(5), ui_rx.recv())
        .await
        .expect("本地 Effect 应在超时前回灌事件")
        .expect("事件通道不应提前关闭")
}

#[tokio::test]
async fn missing_image_path_falls_back_to_pasted_text() {
    let mut app = test_app();
    let (ui_tx, mut ui_rx) = mpsc::channel(4);
    let decoded_path = "/Users/me/Library/Application Support/stickers/a.gif".to_string();
    let pasted_text = "/Users/me/Library/Application\\ Support/stickers/a.gif".to_string();

    app.process_image_file_effect(decoded_path, pasted_text.clone(), &ui_tx);

    let event = recv_event(&mut ui_rx).await;
    assert!(
        matches!(event, UiEvent::PasteFallbackToText { text } if text == pasted_text),
        "路径不存在时必须回填原始粘贴文本，而不是报错"
    );
}

#[tokio::test]
async fn existing_image_path_emits_clipboard_image_with_display_path() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("shot.png");
    std::fs::write(&path, b"png-bytes").expect("write image fixture");
    let display_path = path.to_str().expect("utf8 path").to_string();
    let mut app = test_app();
    let (ui_tx, mut ui_rx) = mpsc::channel(4);

    app.process_image_file_effect(display_path.clone(), display_path.clone(), &ui_tx);

    let event = recv_event(&mut ui_rx).await;
    match event {
        UiEvent::ClipboardImage(view) => {
            assert_eq!(view.display_path.as_deref(), Some(display_path.as_str()));
            assert_eq!(view.media_type, "image/png");
            assert_eq!(view.final_size, "png-bytes".len());
        }
        other => panic!("期望 ClipboardImage，实际 {other:?}"),
    }
}

#[tokio::test]
async fn directory_path_is_treated_as_missing_image() {
    let directory = tempdir().expect("temporary directory");
    let directory_path = directory.path().to_str().expect("utf8 path").to_string();
    let mut app = test_app();
    let (ui_tx, mut ui_rx) = mpsc::channel(4);

    app.process_image_file_effect(directory_path.clone(), directory_path.clone(), &ui_tx);

    let event = recv_event(&mut ui_rx).await;
    assert!(
        matches!(event, UiEvent::PasteFallbackToText { text } if text == directory_path),
        "目录不是图片文件，必须回填原始文本"
    );
}

/// 终端把含空格的本地图片转义成 `Application\ Support/...` 粘贴进来。
/// 解码后的路径必须能命中真实文件，走 ClipboardImage 而不是降级或报错。
#[tokio::test]
async fn escaped_space_path_loads_existing_image_after_decode() {
    let directory = tempdir().expect("temporary directory");
    let spaced_directory = directory.path().join("Application Support");
    std::fs::create_dir(&spaced_directory).expect("create spaced directory");
    let image_path = spaced_directory.join("sticker.gif");
    std::fs::write(&image_path, b"gif-bytes").expect("write image fixture");
    let decoded_path = image_path.to_str().expect("utf8 path").to_string();
    let pasted_text = decoded_path.replace(' ', "\\ ");
    let mut app = test_app();
    let (ui_tx, mut ui_rx) = mpsc::channel(4);

    app.process_image_file_effect(decoded_path.clone(), pasted_text, &ui_tx);

    let event = recv_event(&mut ui_rx).await;
    match event {
        UiEvent::ClipboardImage(view) => {
            assert_eq!(view.display_path.as_deref(), Some(decoded_path.as_str()));
            assert_eq!(view.media_type, "image/gif");
        }
        other => panic!("期望 ClipboardImage，实际 {other:?}"),
    }
}

/// 端到端降级：不可用路径的事件回灌到输入区后，用户原始粘贴文本必须完整可见。
#[tokio::test]
async fn unavailable_image_path_event_restores_pasted_text_into_input() {
    let mut app = test_app();
    let (ui_tx, mut ui_rx) = mpsc::channel(4);
    let pasted_text = "/definitely/missing/photo.png".to_string();

    app.process_image_file_effect(pasted_text.clone(), pasted_text.clone(), &ui_tx);

    let event = recv_event(&mut ui_rx).await;
    let (event_tx, _event_rx) = mpsc::channel(1);
    app.update(
        crate::tui::update::msg::TuiMsg::Ui(event),
        &event_tx,
        &SpawnContextRefs { agent_client: None },
    );

    assert_eq!(app.model.input.document.buffer, pasted_text);
}
