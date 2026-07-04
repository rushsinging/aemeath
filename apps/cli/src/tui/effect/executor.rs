use crate::tui::app::event::UiEvent;
use crate::tui::app::App;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::effect::session::processing;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::runtime::status_notice::StatusNotice;
use tokio::sync::mpsc;

impl App {
    pub(crate) fn execute_spawn_effect(&mut self, effect: SpawnAgentChatEffect) {
        if let Some(spawn_ctx) = effect.context {
            let handle = processing::spawn_processing(spawn_ctx);
            self.chat.set_processing_handle(handle);
        }
    }

    pub(crate) async fn execute_effect(&mut self, effect: Effect, ui_tx: &mpsc::Sender<UiEvent>) {
        match effect {
            Effect::None | Effect::RequestRender => {}
            Effect::QuitApplication => {
                // #390 A1 常驻 loop shutdown：drop input_event_tx → loop 干净退出 →
                // spawn task 执行 auto-save。退出路径在 session_lifecycle.rs 中 await 完成。
                self.chat.clear_input_event_buffer();
                self.layout.request_exit();
            }
            Effect::SpawnAgentChat { .. } => {}
            Effect::SendChatInputEvent { event } => self.send_chat_input_event(event),
            Effect::CancelAgentChat => self.cancel_agent_chat(),
            Effect::SaveSession { notify } => self.save_session_effect(notify, ui_tx),
            Effect::RunHook { message, name } => self.run_hook_effect(message, name),
            Effect::ReadClipboardImage => self.read_clipboard_image_effect(ui_tx),
            Effect::ProcessImageFile { path } => self.process_image_file_effect(path, ui_tx),
            Effect::SetCurrentTurn { turn } => self.set_current_turn_effect(turn),
            Effect::FetchReminderRecap => self.fetch_reminder_recap_effect(ui_tx),
            Effect::FetchMemoryList => self.fetch_memory_list_effect(ui_tx),
            Effect::RunReflection { foreground } => self.run_reflection_effect(foreground, ui_tx),
            Effect::ApplyReflection { output } => self.apply_reflection_effect(output, ui_tx),
            Effect::CopyToClipboard { text } => self.copy_to_clipboard_effect(&text),
            Effect::FetchTaskStatus => self.update_task_status(self.chat.is_processing).await,
            Effect::StartTimer { .. } | Effect::StopTimer { .. } => {}
            Effect::RunSelfUpdate => self.run_self_update_effect(ui_tx).await,
            Effect::ResetRuntimeState => self.reset_runtime_state().await,
        }
    }

    fn cancel_agent_chat(&mut self) {
        self.chat.start_cancelling();
        if let Some(ref ac) = self.agent_client {
            ac.cancel();
        }
        self.model
            .conversation
            .apply(SetStatusNotice(StatusNotice::warning(
                "Cancelling current response… Press Ctrl+C again to exit",
            )));
    }

    fn send_chat_input_event(&mut self, event: sdk::ChatInputEvent) {
        if self.chat.input_event_tx.is_none() {
            crate::tui::log_debug!(
                "send_chat_input_event DROPPED tx=None event={:?}",
                std::mem::discriminant(&event)
            );
            self.append_error_notice("当前 Chat 输入通道不可用，已保留在队列中等待兜底 drain");
            return;
        }
        crate::tui::log_debug!(
            "send_chat_input_event sending event={:?}",
            std::mem::discriminant(&event)
        );
        self.chat.push_input_event(event);
    }

    /// 保存当前会话（/save 与 MessagesSync 共用）。当 `notify=true`（来自 /save）时，    /// 经 UiEvent 回灌成功/失败反馈行，保持原 `[session saved: id]` / `Failed` 体验；
    /// 后台自动保存（MessagesSync）静默。
    fn save_session_effect(&mut self, notify: bool, ui_tx: &mpsc::Sender<UiEvent>) {
        // 后台自动保存（notify=false）在无消息时静默跳过，避免空会话写盘与噪声。
        if !notify && self.chat.messages.is_empty() {
            return;
        }
        let Some(ac) = self.agent_client.clone() else {
            if notify {
                let tx = ui_tx.clone();
                crate::tui::effect::spawn_guard::spawn_guarded("save_session", async move {
                    let _ = tx
                        .send(UiEvent::SlashCommandFailed {
                            message: "Failed to save session: SDK agent client is unavailable"
                                .to_string(),
                        })
                        .await;
                });
            }
            return;
        };
        let messages = self.chat.messages.clone();
        let session_id = self.session.session_id().to_string();
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("save_session", async move {
            if let Err(e) = ac.sync_current_messages(messages).await {
                crate::tui::log_warn!("sync failed: {e}");
            }
            match ac.save_current_session().await {
                Ok(()) => {
                    if notify {
                        let _ = tx.send(UiEvent::SessionSaved { id: session_id }).await;
                    }
                }
                Err(e) => {
                    crate::tui::log_warn!("save failed: {e}");
                    if notify {
                        let _ = tx
                            .send(UiEvent::SlashCommandFailed {
                                message: format!("Failed to save session: {e}"),
                            })
                            .await;
                    }
                }
            }
        });
    }

    fn fetch_memory_list_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        let Some(ac) = self.agent_client.clone() else {
            return;
        };
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("fetch_memory_list", async move {
            match ac.list_reminders().await {
                Ok(reminders) => {
                    let _ = tx.send(UiEvent::MemoryList(reminders)).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(UiEvent::SlashCommandFailed {
                            message: format!("获取 reminders 失败: {e}"),
                        })
                        .await;
                }
            }
        });
    }

    fn run_hook_effect(&mut self, message: String, name: String) {
        let Some(ac) = self.agent_client.clone() else {
            return;
        };
        crate::tui::effect::spawn_guard::spawn_guarded("run_hook", async move {
            let _ = ac.notify_hook(&message, &name).await;
        });
    }

    fn read_clipboard_image_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        let Some(ac) = self.agent_client.clone() else {
            return;
        };
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("clipboard_image", async move {
            match ac.read_clipboard_image().await {
                Ok(img) => {
                    let _ = tx.send(UiEvent::ClipboardImage(img)).await;
                }
                Err(e) => crate::tui::log_warn!("clipboard read failed: {e}"),
            }
        });
    }

    fn process_image_file_effect(&mut self, path: String, ui_tx: &mpsc::Sender<UiEvent>) {
        let Some(ac) = self.agent_client.clone() else {
            return;
        };
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("image_file", async move {
            match ac.process_image_file(path).await {
                Ok(img) => {
                    let _ = tx.send(UiEvent::ClipboardImage(img)).await;
                }
                Err(e) => crate::tui::log_warn!("image process failed: {e}"),
            }
        });
    }

    /// 将文本复制到系统剪贴板，并据结果在 status bar 给出临时反馈。
    fn copy_to_clipboard_effect(&mut self, text: &str) {
        match crate::tui::render::input::clipboard::copy_text(text) {
            Ok(()) => {
                self.set_transient_notice(StatusNotice::success("已复制选中内容"));
            }
            Err(err) => {
                crate::tui::log_warn!("复制选中内容失败: {err}");
                self.set_transient_notice(StatusNotice::warning(err));
            }
        }
    }

    fn set_current_turn_effect(&mut self, turn: usize) {
        if let Some(ref ac) = self.agent_client {
            ac.set_current_turn(turn);
        }
    }

    /// 执行 LLM reflection：克隆当前消息与 agent client，后台 spawn 调用 SDK，
    /// 结果经 UiEvent 回流到 update。前台发起时先推送 ReflectionStarted。
    fn run_reflection_effect(&mut self, foreground: bool, ui_tx: &mpsc::Sender<UiEvent>) {
        let Some(agent_client) = self.agent_client.clone() else {
            return;
        };
        let messages = self.chat.messages.clone();
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("reflection", async move {
            if foreground {
                let _ = tx.send(UiEvent::ReflectionStarted).await;
            }
            match agent_client.run_reflection(messages).await {
                Ok(output) => {
                    let _ = tx.send(UiEvent::ReflectionUsage).await;
                    let _ = tx.send(UiEvent::ReflectionDone { output }).await;
                }
                Err(error) => {
                    let _ = tx
                        .send(UiEvent::Error(format!("Reflection LLM 调用失败: {error}")))
                        .await;
                }
            }
        });
    }

    /// 将 reflection 输出应用到 SDK memory 能力（后台 spawn）。
    fn apply_reflection_effect(
        &mut self,
        output: sdk::ReflectionOutputView,
        ui_tx: &mpsc::Sender<UiEvent>,
    ) {
        let Some(agent_client) = self.agent_client.clone() else {
            return;
        };
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("apply_reflection", async move {
            let result = agent_client
                .apply_reflection(output.clone())
                .await
                .map_err(|error| error.to_string());
            let _ = tx
                .send(UiEvent::ReflectionApplyDone { output, result })
                .await;
        });
    }

    /// 启动时后台检查版本更新（非阻塞）。
    /// 使用 `force_check` 忽略缓存，确保每次启动都查最新状态。
    /// 失败时静默降级——版本检查不应影响正常使用。
    pub(crate) fn spawn_update_check(&self, ui_tx: mpsc::Sender<UiEvent>) {
        let service = composition::update::wire_update();
        crate::tui::effect::spawn_guard::spawn_guarded("update_check", async move {
            match service.force_check().await {
                Ok(check) if check.is_update_available => {
                    let _ = ui_tx
                        .send(UiEvent::UpdateAvailable {
                            current: check.current_version,
                            latest: check.latest_version,
                            release_url: check.release_url,
                        })
                        .await;
                }
                Ok(_) => {} // 已是最新，不提示
                Err(e) => {
                    crate::tui::log_warn!("版本检查失败（已忽略）: {e}");
                }
            }
        });
    }

    /// `/update` 命令触发的自动更新执行器。
    /// 在后台执行 perform_update，结果通过 UiEvent 回灌。
    async fn run_self_update_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        self.append_system_notice("[checking for updates...]".to_string());
        let service = composition::update::wire_update();
        let ui_tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("self_update", async move {
            match service.perform_update().await {
                Ok(sdk::UpdateResult::Updated {
                    from,
                    to,
                    installed_path,
                }) => {
                    let _ = ui_tx
                        .send(UiEvent::SystemMessage(format!(
                            "✓ Updated aemeath {from} → {to}\nInstalled to: {installed_path}\nPlease restart aemeath to use the new version."
                        )))
                        .await;
                }
                Ok(sdk::UpdateResult::UpToDate { version }) => {
                    let _ = ui_tx
                        .send(UiEvent::SystemMessage(format!(
                            "Already up to date ({version})."
                        )))
                        .await;
                }
                Ok(sdk::UpdateResult::CheckOnly(_)) => {}
                Err(e) => {
                    let _ = ui_tx
                        .send(UiEvent::SystemMessage(format!("Update failed: {e}")))
                        .await;
                }
            }
        });
    }

    fn fetch_reminder_recap_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        let Some(ac) = self.agent_client.clone() else {
            return;
        };
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("fetch_reminder_recap", async move {
            match ac.list_reminders().await {
                Ok(reminders) => {
                    if let Some(line) = sdk::ReminderView::recap_line(&reminders) {
                        let _ = tx.send(UiEvent::ReminderRecap(line)).await;
                    }
                }
                Err(e) => {
                    crate::tui::log_warn!("fetch reminder recap failed: {e}")
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_runtime_ignores_noop_effect() {
        let app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        assert!(!app.layout.should_exit);
    }

    #[test]
    fn test_effect_runtime_quit_effect_sets_exit_flag() {
        let mut app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        app.layout.request_exit();
        assert!(app.layout.should_exit);
    }

    #[test]
    fn test_effect_runtime_accepts_pending_image() {
        let mut app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        // accept_pending_clipboard_image 已移除（#497 spawn_guarded 化），
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
}
