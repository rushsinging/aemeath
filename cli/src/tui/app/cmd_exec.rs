use super::{processing, App, UiEvent};
use crate::tui::app::msg::Cmd;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl App {
    /// Execute a single Cmd (recursive for Batch).
    pub(super) async fn exec_one_cmd(
        app: &mut App,
        active_cancel: &std::sync::Arc<std::sync::Mutex<Option<CancellationToken>>>,
        ui_tx: &mpsc::Sender<UiEvent>,
        cmd: Cmd,
    ) {
        match cmd {
            Cmd::None => {}
            Cmd::Quit => {
                app.should_exit = true;
            }
            Cmd::SpawnProcessing(ctx) => {
                if let Ok(mut guard) = active_cancel.lock() {
                    *guard = Some(ctx.cancel.clone());
                }
                processing::spawn_processing(ctx);
            }
            Cmd::SendEvents(events) => {
                for ev in events {
                    let _ = ui_tx.send(ev).await;
                }
            }
            Cmd::QueueInput(_) => {}
            Cmd::SaveSession(msgs) => {
                if !msgs.is_empty() {
                    let s = app.build_session(msgs).await;
                    if let Err(e) = aemeath_core::session::save_session(&s).await {
                        log::warn!("failed to auto-save session on sync: {e}");
                    }
                }
            }
            Cmd::RunHookNotification { message, kind } => {
                let hook_runner = app.hook_runner.clone();
                tokio::spawn(async move {
                    let _ = hook_runner.on_notification(&message, &kind).await;
                });
            }
            Cmd::ReadClipboardImage => {
                let tx = ui_tx.clone();
                tokio::spawn(async move {
                    match crate::image::read_clipboard_image().await {
                        Ok(img) => {
                            let size = img.final_size;
                            let _ = tx.send(UiEvent::ClipboardImage(img)).await;
                            let _ = tx
                                .send(UiEvent::SystemMessage(format!(
                                    "[clipboard image added ({} bytes). Type message to send.]",
                                    size
                                )))
                                .await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(UiEvent::SystemMessage(format!(
                                    "No image in clipboard: {e}"
                                )))
                                .await;
                        }
                    }
                });
            }
            Cmd::ProcessImageFile(path) => {
                let tx = ui_tx.clone();
                tokio::spawn(async move {
                    match crate::image::process_image_file(&path).await {
                        Ok(img) => {
                            let size = img.final_size;
                            let _ = tx.send(UiEvent::ClipboardImage(img)).await;
                            let _ = tx
                                .send(UiEvent::SystemMessage(format!(
                                    "[image loaded ({} bytes). Type message to send.]",
                                    size
                                )))
                                .await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(UiEvent::SystemMessage(format!("Failed to load image: {e}")))
                                .await;
                        }
                    }
                });
            }
        }
    }
}
