use super::App;
use crate::tui::core::event::UiEvent;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::session::processing;
use crate::tui::update::msg::TuiMsg;
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

impl App {
    pub(crate) async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        interrupted: Arc<AtomicBool>,
    ) -> io::Result<()> {
        let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(256);
        self.chat.stop_processing();

        let mut event_stream = EventStream::new();
        let mut spinner_ticker = tokio::time::interval(std::time::Duration::from_millis(90));
        spinner_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            // Update task status lines
            self.update_task_status(self.chat.is_processing).await;

            // Ctrl+C 超时复原 status line
            self.check_ctrlc_timeout();

            // Draw UI
            self.draw(terminal)?;

            let spawn_refs = processing::SpawnContextRefs {
                agent_client: self.agent_client.clone(),
            };

            // --- TEA event collection: produce a TuiMsg ---
            let msg: Option<TuiMsg> = tokio::select! {
                biased;
                ev = ui_rx.recv() => { ev.map(TuiMsg::Ui) }
                ev = event_stream.next() => {
                    match ev {
                        Some(Ok(event)) => match event {
                            Event::Paste(text) => Some(TuiMsg::Paste(text)),
                            Event::Mouse(mouse) => Some(TuiMsg::Mouse(mouse)),
                            Event::Key(key) => Some(TuiMsg::Key(key)),
                            Event::Resize(width, height) => Some(TuiMsg::Resize { width, height }),
                            _ => None,
                        },
                        _ => None,
                    }
                }
                _ = spinner_ticker.tick() => { Some(TuiMsg::SpinnerTick) }
            };

            let Some(msg) = msg else {
                self.input.just_pasted = false;
                continue;
            };

            // --- TEA update: state transition ---
            let result = self.update(msg, &ui_tx, &spawn_refs);

            // --- Handle pending slash commands (async) ---
            if let Some(input) = result.pending_slash {
                let review_prompt = self
                    .handle_slash_command_with_events(&input, Some(ui_tx.clone()))
                    .await;
                if let Some(prompt) = review_prompt {
                    self.output_area.push_user_message(&input);
                    self.chat
                        .messages
                        .push(sdk::ChatMessage::user_text(&prompt));
                    interrupted.store(false, Ordering::Relaxed);
                    self.output_area.start_spinner();
                    self.output_area.set_spinner_phase("Thinking...");
                    self.chat.start_processing();
                    if let Some(spawn_ctx) = self.build_spawn_context(&ui_tx, &spawn_refs) {
                        processing::spawn_processing(spawn_ctx);
                    } else {
                        self.output_area
                            .push_error("SDK agent client is unavailable");
                    }
                }
            }

            if let Some(spawn_effect) = result.spawn_effect {
                self.execute_spawn_effect(spawn_effect);
            }

            // --- TEA effect execution: handle side effects inline via AgentClient ---
            for effect in result.effects {
                self.execute_effect(effect, &ui_tx).await;
            }

            self.input.just_pasted = false;
            if self.layout.should_exit {
                break;
            }
        }
        Ok(())
    }

    fn execute_spawn_effect(&mut self, effect: SpawnAgentChatEffect) {
        if let Some(spawn_ctx) = effect.context {
            processing::spawn_processing(spawn_ctx);
        }
    }

    async fn execute_effect(&mut self, effect: Effect, ui_tx: &mpsc::Sender<UiEvent>) {
        match effect {
            Effect::None | Effect::RequestRender => {}
            Effect::QuitApplication => self.layout.request_exit(),
            Effect::SpawnAgentChat { .. } => {}
            Effect::CancelAgentChat => {
                if let Some(ref ac) = self.agent_client {
                    ac.cancel();
                }
            }
            Effect::SaveSession => {
                if !self.chat.messages.is_empty() {
                    if let Some(ref ac) = self.agent_client {
                        if let Err(e) = ac.sync_current_messages(self.chat.messages.clone()).await {
                            log::warn!("sync failed: {e}");
                        }
                        if let Err(e) = ac.save_current_session().await {
                            log::warn!("save failed: {e}");
                        }
                    }
                }
            }
            Effect::RunHook { message, name } => {
                if let Some(ref ac) = self.agent_client {
                    let _ = ac.notify_hook(&message, &name).await;
                }
            }
            Effect::ReadClipboardImage => {
                if let Some(ref ac) = self.agent_client {
                    match ac.read_clipboard_image().await {
                        Ok(img) => {
                            let count = self.chat.add_pending_image(img);
                            self.input_area.set_pending_images(count);
                        }
                        Err(e) => log::warn!("clipboard read failed: {e}"),
                    }
                }
            }
            Effect::ProcessImageFile { path } => {
                if let Some(ref ac) = self.agent_client {
                    match ac.process_image_file(path).await {
                        Ok(img) => {
                            let count = self.chat.add_pending_image(img);
                            self.input_area.set_pending_images(count);
                        }
                        Err(e) => log::warn!("image process failed: {e}"),
                    }
                }
            }
            Effect::SetCurrentTurn { turn } => {
                if let Some(ref ac) = self.agent_client {
                    ac.set_current_turn(turn);
                }
            }
            Effect::FetchReminderRecap => {
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
            Effect::FetchTaskStatus
            | Effect::CopyToClipboard { .. }
            | Effect::StartTimer { .. }
            | Effect::StopTimer { .. } => {}
        }
    }
}
