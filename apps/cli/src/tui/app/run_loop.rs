use super::App;
use crate::tui::app::event::UiEvent;
use crate::tui::effect::session::processing;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;
use crate::tui::update::msg::TuiMsg;
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use sdk::ChangeSet;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

impl App {
    async fn handle_change_set(&mut self, change: ChangeSet) {
        if change.contains(ChangeSet::TASKS) {
            self.update_task_status(self.chat.is_processing).await;
        }
        if change.contains(ChangeSet::PROJECT) {
            self.update_project_context().await;
        }
    }

    pub(crate) async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        interrupted: Arc<AtomicBool>,
    ) -> io::Result<()> {
        let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(256);
        self.chat.stop_processing();

        let mut event_stream = EventStream::new();
        let mut change_rx = self.agent_client.as_ref().map(|client| client.changes());
        let mut spinner_ticker = tokio::time::interval(std::time::Duration::from_millis(90));
        spinner_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        // 首帧渲染先建立 layout 尺寸，再按真实宽度刷新启动横幅 document。
        self.update_task_status(self.chat.is_processing).await;
        self.update_project_context().await;
        self.draw(terminal)?;
        self.refresh_output_document_from_model();

        loop {
            // Ctrl+C 超时复原 status line
            self.check_ctrlc_timeout();

            // 每帧先批量派生 dirty ViewModel，避免 streaming chunk 每次同步重渲染输出区。
            self.flush_dirty_view_models();
            // 每帧维护 live-status 动画 view_state；render 直接消费 LiveStatusViewModel。
            self.refresh_live_status_from_model();
            // 每帧据 layout/live-status 与 document 指标同步 view_state 滚动真相。
            self.refresh_output_scroll_from_view_state();
            // Draw UI
            self.draw(terminal)?;

            let spawn_refs = processing::SpawnContextRefs {
                agent_client: self.agent_client.clone(),
            };

            // --- TEA event collection: produce a TuiMsg ---
            let msg: Option<TuiMsg> = tokio::select! {
                biased;
                ev = ui_rx.recv() => { ev.map(TuiMsg::Ui) }
                change = async {
                    match change_rx.as_mut() {
                        Some(rx) => rx.changed().await.ok().map(|_| *rx.borrow()),
                        None => futures::future::pending().await,
                    }
                } => {
                    if let Some(change) = change {
                        self.handle_change_set(change).await;
                    }
                    None
                }
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
                    self.model
                        .conversation
                        .apply(ConversationIntent::StartChat {
                            submission: input.clone(),
                        });
                    self.mark_output_dirty();
                    self.chat
                        .messages
                        .push(sdk::ChatMessage::user_text(&prompt));
                    interrupted.store(false, Ordering::Relaxed);
                    self.spinner_phase(SpinnerPhase::Thinking);
                    self.chat.start_processing();
                    if let Some(spawn_ctx) = self.build_spawn_context(&ui_tx, &spawn_refs) {
                        let handle = processing::spawn_processing(spawn_ctx);
                        self.chat.set_processing_handle(handle);
                    } else {
                        self.append_error_notice("SDK agent client is unavailable");
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
}
