use super::App;
use crate::tui::core::event::UiEvent;
use crate::tui::core::msg::{Cmd, Msg};
use crate::tui::session::processing;
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
        _client: Arc<::runtime::api::provider::client::LlmClient>,
        _registry: Arc<::runtime::api::core::tool::ToolRegistry>,
        _system_blocks: Vec<::runtime::api::provider::types::SystemBlock>,
        _system_prompt_text: String,
        _user_context: String,
        _context_size: usize,
        _verbose: bool,
        _agent_runner: Option<Arc<dyn ::runtime::api::core::tool::AgentRunner>>,
        _allow_all: bool,
        interrupted: Arc<AtomicBool>,
        task_store: Arc<::runtime::api::core::task::TaskStore>,
        _max_tool_concurrency: usize,
        _max_agent_concurrency: usize,
        _agent_semaphore: Arc<tokio::sync::Semaphore>,
    ) -> io::Result<()> {
        let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(256);
        self.chat.is_processing = false;

        let mut event_stream = EventStream::new();
        let mut spinner_ticker = tokio::time::interval(std::time::Duration::from_millis(90));
        spinner_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            // Update task status lines
            self.update_task_status(&task_store, self.chat.is_processing)
                .await;

            // Ctrl+C 超时复原 status line
            self.check_ctrlc_timeout();

            // Draw UI
            self.draw(terminal)?;

            let spawn_refs = processing::SpawnContextRefs {
                agent_client: self.agent_client.clone(),
            };

            // --- TEA event collection: produce a Msg ---
            let msg: Option<Msg> = tokio::select! {
                biased;
                // UI events have highest priority
                ev = ui_rx.recv() => {
                    ev.map(Msg::Ui)
                }
                // Terminal events
                ev = event_stream.next() => {
                    match ev {
                        Some(Ok(event)) => match event {
                            Event::Paste(text) => Some(Msg::Paste(text)),
                            Event::Mouse(mouse) => Some(Msg::Mouse(mouse)),
                            Event::Key(key) => Some(Msg::Key(key)),
                            Event::Resize(width, height) => Some(Msg::Resize { width, height }),
                            _ => None,
                        },
                        _ => None,
                    }
                }
                // Fixed ticker for spinner animation. The frame advances only here,
                // so stream/tool events and redraw frequency cannot speed it up.
                _ = spinner_ticker.tick() => {
                    Some(Msg::SpinnerTick)
                }
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
                    self.chat.is_processing = true;
                    if let Some(spawn_ctx) = self.build_spawn_context(&ui_tx, &spawn_refs) {
                        processing::spawn_processing(spawn_ctx);
                    } else {
                        self.output_area
                            .push_error("SDK agent client is unavailable");
                    }
                }
            }

            // --- TEA command execution ---
            // Handle &mut App cases first (Quit, SaveCurrentSession) to avoid borrow conflicts
            match &result.cmd {
                Cmd::Quit => self.layout.should_exit = true,
                Cmd::SaveCurrentSession if !self.chat.messages.is_empty() => {
                    if let Some(agent_client) = &self.agent_client {
                        if let Err(e) = agent_client
                            .sync_current_messages(self.chat.messages.clone())
                            .await
                        {
                            log::warn!("failed to sync session messages: {e}");
                        }
                        if let Err(e) = agent_client.save_current_session().await {
                            log::warn!("failed to auto-save session on sync: {e}");
                        }
                    } else {
                        log::warn!(
                            "failed to auto-save session on sync: SDK agent client is unavailable"
                        );
                    }
                }
                _ => self.cmd_exec.exec_one_cmd(&ui_tx, result.cmd).await,
            }

            self.input.just_pasted = false;
            if self.layout.should_exit {
                break;
            }
        }
        Ok(())
    }
}
