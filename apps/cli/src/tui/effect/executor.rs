use crate::tui::app::event::UiEvent;
use crate::tui::app::App;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::effect::session::processing;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::status_notice::StatusNotice;
use tokio::sync::mpsc;

impl App {
    pub(crate) fn execute_spawn_effect(&mut self, effect: SpawnAgentChatEffect) {
        if let Some(spawn_ctx) = effect.context {
            processing::spawn_processing(spawn_ctx);
        }
    }

    pub(crate) async fn execute_effect(&mut self, effect: Effect, ui_tx: &mpsc::Sender<UiEvent>) {
        match effect {
            Effect::None | Effect::RequestRender => {}
            Effect::QuitApplication => self.layout.request_exit(),
            Effect::SpawnAgentChat { .. } => {}
            Effect::SendChatInputEvent { event } => self.send_chat_input_event(event),
            Effect::CancelAgentChat => self.cancel_agent_chat(),
            Effect::SaveSession { notify } => self.save_session_effect(notify, ui_tx).await,
            Effect::RunHook { message, name } => self.run_hook_effect(message, name).await,
            Effect::ReadClipboardImage => self.read_clipboard_image_effect().await,
            Effect::ProcessImageFile { path } => self.process_image_file_effect(path).await,
            Effect::SetCurrentTurn { turn } => self.set_current_turn_effect(turn),
            Effect::FetchReminderRecap => self.fetch_reminder_recap_effect(ui_tx).await,
            Effect::FetchMemoryList => self.fetch_memory_list_effect(ui_tx).await,
            Effect::RunReflection { foreground } => self.run_reflection_effect(foreground, ui_tx),
            Effect::ApplyReflection { output } => self.apply_reflection_effect(output),
            Effect::CopyToClipboard { text } => self.copy_to_clipboard_effect(&text),
            Effect::FetchTaskStatus => self.update_task_status(self.chat.is_processing).await,
            Effect::StartTimer { .. } | Effect::StopTimer { .. } => {}
        }
    }

    fn cancel_agent_chat(&mut self) {
        if let Some(ref ac) = self.agent_client {
            ac.cancel();
        }
    }

    fn send_chat_input_event(&mut self, event: sdk::ChatInputEvent) {
        if self.chat.input_event_buffer.is_none() {
            self.append_error_notice("当前 Chat 输入通道不可用，已保留在队列中等待兜底 drain");
            return;
        }
        self.chat.push_input_event(event);
    }

    /// 保存当前会话（/save 与 MessagesSync 共用）。当 `notify=true`（来自 /save）时，    /// 经 UiEvent 回灌成功/失败反馈行，保持原 `[session saved: id]` / `Failed` 体验；
    /// 后台自动保存（MessagesSync）静默。
    async fn save_session_effect(&mut self, notify: bool, ui_tx: &mpsc::Sender<UiEvent>) {
        // 后台自动保存（notify=false）在无消息时静默跳过，避免空会话写盘与噪声。
        if !notify && self.chat.messages.is_empty() {
            return;
        }
        let Some(ac) = self.agent_client.clone() else {
            if notify {
                let _ = ui_tx
                    .send(UiEvent::SlashCommandFailed {
                        message: "Failed to save session: SDK agent client is unavailable"
                            .to_string(),
                    })
                    .await;
            }
            return;
        };
        if let Err(e) = ac.sync_current_messages(self.chat.messages.clone()).await {
            log::warn!("sync failed: {e}");
        }
        match ac.save_current_session().await {
            Ok(()) => {
                if notify {
                    let _ = ui_tx
                        .send(UiEvent::SessionSaved {
                            id: self.session.session_id().to_string(),
                        })
                        .await;
                }
            }
            Err(e) => {
                log::warn!("save failed: {e}");
                if notify {
                    let _ = ui_tx
                        .send(UiEvent::SlashCommandFailed {
                            message: format!("Failed to save session: {e}"),
                        })
                        .await;
                }
            }
        }
    }

    async fn fetch_memory_list_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        if let Some(ref ac) = self.agent_client {
            match ac.list_reminders().await {
                Ok(reminders) => {
                    let _ = ui_tx.send(UiEvent::MemoryList(reminders)).await;
                }
                Err(e) => {
                    let _ = ui_tx
                        .send(UiEvent::SlashCommandFailed {
                            message: format!("获取 reminders 失败: {e}"),
                        })
                        .await;
                }
            }
        }
    }

    async fn run_hook_effect(&mut self, message: String, name: String) {
        if let Some(ref ac) = self.agent_client {
            let _ = ac.notify_hook(&message, &name).await;
        }
    }

    async fn read_clipboard_image_effect(&mut self) {
        if let Some(ref ac) = self.agent_client {
            match ac.read_clipboard_image().await {
                Ok(img) => self.accept_pending_clipboard_image(img),
                Err(e) => log::warn!("clipboard read failed: {e}"),
            }
        }
    }

    async fn process_image_file_effect(&mut self, path: String) {
        if let Some(ref ac) = self.agent_client {
            match ac.process_image_file(path).await {
                Ok(img) => self.accept_pending_clipboard_image(img),
                Err(e) => log::warn!("image process failed: {e}"),
            }
        }
    }

    fn accept_pending_clipboard_image(&mut self, img: sdk::ClipboardImageView) {
        let count = self.chat.add_pending_image(img);
        self.handle_input_intent(
            crate::tui::model::input::intent::InputIntent::SetAttachmentCount(count),
        );
    }

    /// 将文本复制到系统剪贴板，并据结果在 status bar 给出反馈。
    fn copy_to_clipboard_effect(&mut self, text: &str) {
        match crate::tui::render::input::clipboard::copy_text(text) {
            Ok(()) => {
                self.model
                    .runtime
                    .apply(RuntimeIntent::SetStatusNotice(StatusNotice::success(
                        "已复制选中内容",
                    )));
            }
            Err(err) => {
                log::warn!("复制选中内容失败: {err}");
                self.model
                    .runtime
                    .apply(RuntimeIntent::SetStatusNotice(StatusNotice::warning(err)));
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
        tokio::spawn(async move {
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
    fn apply_reflection_effect(&mut self, output: sdk::ReflectionOutputView) {
        let Some(agent_client) = self.agent_client.clone() else {
            return;
        };
        tokio::spawn(async move {
            let _ = agent_client.apply_reflection(output).await;
        });
    }

    async fn fetch_reminder_recap_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        if let Some(ref ac) = self.agent_client {
            match ac.list_reminders().await {
                Ok(reminders) => {
                    if let Some(line) = sdk::ReminderView::recap_line(&reminders) {
                        let _ = ui_tx.send(UiEvent::ReminderRecap(line)).await;
                    }
                }
                Err(e) => log::warn!("fetch reminder recap failed: {e}"),
            }
        }
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
        app.accept_pending_clipboard_image(sdk::ClipboardImageView {
            base64: "abc".to_string(),
            media_type: "image/png".to_string(),
            final_size: 3,
            display_path: None,
            width: None,
            height: None,
        });
        assert_eq!(app.chat.pending_images().len(), 1);
    }
}
