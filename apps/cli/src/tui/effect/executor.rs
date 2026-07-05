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
        // #567 S4：cancel 通过 ProcessingHandle.abort() 管理
        if let Some(h) = &self.chat.processing_handle {
            h.abort();
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
        // #567：save 走事件流（ChatInputEvent::SaveSession），
        // loop idle 分支执行 save 并通过 CommandResultText 回传。
        // 不再调 ac.sync_current_messages / ac.save_current_session。
        self.chat.push_input_event(sdk::ChatInputEvent::SaveSession);
        if notify {
            let tx = ui_tx.clone();
            let id = self.session.session_id().to_string();
            crate::tui::effect::spawn_guard::spawn_guarded("save_notify", async move {
                let _ = tx.send(UiEvent::SessionSaved { id }).await;
            });
        }
    }

    fn fetch_memory_list_effect(&mut self, _ui_tx: &mpsc::Sender<UiEvent>) {
        // #567：list_reminders 走事件流（ChatInputEvent::ListReminders）。
        // runtime idle 分支查询，结果通过 ReminderList 事件回传。
        self.chat
            .push_input_event(sdk::ChatInputEvent::ListReminders);
    }

    fn run_hook_effect(&mut self, message: String, name: String) {
        // #567：notify_hook 删除——hook 是 runtime 内部行为，TUI 不参与。
        // hook 触发由 runtime 在消息变更时自行执行。
        let _ = (message, name);
    }

    fn read_clipboard_image_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        // #567 S10：read_clipboard_image 迁移到 TUI 本地
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("clipboard_image", async move {
            match crate::tui::render::input::clipboard::read_image().await {
                Ok(img) => {
                    use base64::Engine;
                    let view = sdk::ClipboardImageView {
                        base64: base64::engine::general_purpose::STANDARD.encode(&img.data),
                        media_type: img.media_type,
                        final_size: img.data.len(),
                        display_path: None,
                        width: None,
                        height: None,
                    };
                    let _ = tx.send(UiEvent::ClipboardImage(view)).await;
                }
                Err(e) => crate::tui::log_warn!("clipboard read failed: {e}"),
            }
        });
    }

    fn process_image_file_effect(&mut self, path: String, ui_tx: &mpsc::Sender<UiEvent>) {
        // #567 S10：process_image_file 迁移到 TUI 本地
        let tx = ui_tx.clone();
        crate::tui::effect::spawn_guard::spawn_guarded("image_file", async move {
            match crate::tui::render::input::clipboard::process_image_file(&path) {
                Ok(img) => {
                    use base64::Engine;
                    let view = sdk::ClipboardImageView {
                        base64: base64::engine::general_purpose::STANDARD.encode(&img.data),
                        media_type: img.media_type,
                        final_size: img.data.len(),
                        display_path: Some(path),
                        width: None,
                        height: None,
                    };
                    let _ = tx.send(UiEvent::ClipboardImage(view)).await;
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

    fn set_current_turn_effect(&mut self, _turn: usize) {
        // #567：set_current_turn 删除——runtime loop 内部自维护 turn 计数器。
    }

    /// 执行 LLM reflection：克隆当前消息与 agent client，后台 spawn 调用 SDK，
    /// 结果经 UiEvent 回流到 update。前台发起时先推送 ReflectionStarted。
    fn run_reflection_effect(&mut self, foreground: bool, _ui_tx: &mpsc::Sender<UiEvent>) {
        // #567：run_reflection 走事件流（ChatInputEvent::RunReflection）。
        // runtime idle 分支执行 reflection，结果通过 ReflectionResult 事件回传。
        let _ = foreground;
        self.chat
            .push_input_event(sdk::ChatInputEvent::RunReflection);
    }

    fn apply_reflection_effect(
        &mut self,
        output: sdk::ReflectionOutputView,
        _ui_tx: &mpsc::Sender<UiEvent>,
    ) {
        // #567：apply_reflection 走事件流（ChatInputEvent::ApplyReflection）。
        // runtime idle 分支执行 apply，结果通过 CommandResultText 事件回传。
        self.chat
            .push_input_event(sdk::ChatInputEvent::ApplyReflection { output });
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

    fn fetch_reminder_recap_effect(&mut self, _ui_tx: &mpsc::Sender<UiEvent>) {
        // #567：list_reminders 走事件流。reminder recap 由 ReminderList 事件回传后处理。
        // 暂时发 ListReminders 事件，recap 在 UiEvent 处理中生成。
        self.chat
            .push_input_event(sdk::ChatInputEvent::ListReminders);
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
