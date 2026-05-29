use super::App;
use crate::tui::app::event::UiEvent;
use crate::tui::effect::session::processing;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;
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

        // 首帧渲染先建立 layout 尺寸，再按真实宽度刷新启动横幅 document。
        self.draw(terminal)?;
        self.refresh_output_widget_from_model();

        loop {
            // Update task status lines
            self.update_task_status(self.chat.is_processing).await;

            // Ctrl+C 超时复原 status line
            self.check_ctrlc_timeout();

            // 每帧据 Model+view_state 派生 spinner/task 镜像，单向写回 widget。
            self.refresh_live_status_from_model();
            // 每帧据 view_state 滚动真相写回 widget 镜像（last_visible_height 反喂 + 钳制）。
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
                    self.refresh_output_widget_from_model();
                    self.chat
                        .messages
                        .push(sdk::ChatMessage::user_text(&prompt));
                    interrupted.store(false, Ordering::Relaxed);
                    self.spinner_phase(SpinnerPhase::Thinking);
                    self.chat.start_processing();
                    if let Some(spawn_ctx) = self.build_spawn_context(&ui_tx, &spawn_refs) {
                        processing::spawn_processing(spawn_ctx);
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
