use super::{processing, UiEvent};
use crate::tui::app::msg::Cmd;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use ::runtime::api::core::config::ModelsConfig;
use ::runtime::api::hook::hook::HookRunner;
use ::runtime::api::storage::logging::JsonLogger;
use ::runtime::api::core::memory::SessionReminders;
use ::runtime::api::core::session::WorkspaceContext;
use ::runtime::api::core::task::TaskStore;

/// 副作用执行器：持有所有 runtime::api 基础设施引用
/// CLI 只依赖 runtime，不直接依赖 llm / core / provider
pub struct CmdExecutor {
    pub client: Option<Arc<::runtime::api::provider::client::LlmClient>>,
    pub models_config: ModelsConfig,
    pub hook_runner: HookRunner,
    pub session_reminders: Arc<std::sync::Mutex<SessionReminders>>,
    pub task_store: Option<Arc<TaskStore>>,
    pub workspace_context: Option<WorkspaceContext>,
    pub json_logger: Option<Arc<std::sync::Mutex<JsonLogger>>>,
}

impl CmdExecutor {
    /// Execute side-effect commands (no &mut App access).
    /// Quit and SaveSession are handled by the caller.
    pub(super) async fn exec_one_cmd(
        &self,
        active_cancel: &std::sync::Arc<std::sync::Mutex<Option<CancellationToken>>>,
        ui_tx: &mpsc::Sender<UiEvent>,
        cmd: Cmd,
    ) {
        match cmd {
            Cmd::None => {}
            Cmd::Quit => {} // handled by caller
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
            Cmd::SaveSession(_) => {} // handled by caller
            Cmd::RunHookNotification { message, kind } => {
                let hook_runner = self.hook_runner.clone();
                tokio::spawn(async move {
                    let _ = hook_runner.on_notification(&message, &kind).await;
                });
            }
            Cmd::ReadClipboardImage => {
                let tx = ui_tx.clone();
                tokio::spawn(async move {
                    match ::runtime::api::image::read_clipboard_image().await {
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
                    match ::runtime::api::image::process_image_file(&path).await {
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
