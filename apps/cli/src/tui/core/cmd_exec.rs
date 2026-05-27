use super::UiEvent;
use crate::tui::core::msg::Cmd;
use crate::tui::session::processing;
use ::runtime::api::core::config::ModelsConfig;
use ::runtime::api::core::task::TaskStore;
use ::runtime::api::core::tool::SessionReminders;
use ::runtime::api::hook::hook::HookRunner;
use ::runtime::api::session::WorkspaceContext;
use std::sync::Arc;
use tokio::sync::mpsc;

/// 副作用执行器：持有所有 runtime::api 基础设施引用
/// CLI 只依赖 runtime，不直接依赖 llm / core / provider
pub struct CmdExecutor {
    pub client: Option<Arc<::runtime::api::provider::client::LlmClient>>,
    pub models_config: ModelsConfig,
    pub hook_runner: HookRunner,
    pub session_reminders: Arc<std::sync::Mutex<SessionReminders>>,
    pub task_store: Option<Arc<TaskStore>>,
    pub workspace_context: Option<WorkspaceContext>,
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
}

impl CmdExecutor {
    /// Execute side-effect commands (no &mut App access).
    /// Quit and SaveCurrentSession are handled by the caller.
    pub(super) async fn exec_one_cmd(&self, ui_tx: &mpsc::Sender<UiEvent>, cmd: Cmd) {
        match cmd {
            Cmd::None => {}
            Cmd::Quit => {} // handled by caller
            Cmd::SpawnProcessing(ctx) => {
                processing::spawn_processing(ctx);
            }
            Cmd::SendEvents(events) => {
                for ev in events {
                    let _ = ui_tx.send(ev).await;
                }
            }
            Cmd::QueueInput(_) => {}
            Cmd::SaveCurrentSession => {} // handled by caller
            Cmd::RunHookNotification { message, kind } => {
                let hook_runner = self.hook_runner.clone();
                tokio::spawn(async move {
                    let _ = hook_runner.on_notification(&message, &kind).await;
                });
            }
            Cmd::ReadClipboardImage => {
                let tx = ui_tx.clone();
                let Some(agent_client) = self.agent_client.clone() else {
                    tokio::spawn(async move {
                        let _ = tx
                            .send(UiEvent::SystemMessage(
                                "No SDK client available for clipboard image".to_string(),
                            ))
                            .await;
                    });
                    return;
                };
                tokio::spawn(async move {
                    match agent_client.read_clipboard_image().await {
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
                let Some(agent_client) = self.agent_client.clone() else {
                    tokio::spawn(async move {
                        let _ = tx
                            .send(UiEvent::SystemMessage(
                                "No SDK client available for image processing".to_string(),
                            ))
                            .await;
                    });
                    return;
                };
                tokio::spawn(async move {
                    match agent_client.process_image_file(path).await {
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
