use super::{processing, App, UiEvent};
use crate::tui::app::msg::{Cmd, Msg};
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl App {
    pub(super) async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        client: Arc<::runtime::api::provider::client::LlmClient>,
        registry: Arc<::runtime::api::core::tool::ToolRegistry>,
        system_blocks: Vec<::runtime::api::provider::types::SystemBlock>,
        system_prompt_text: String,
        user_context: String,
        context_size: usize,
        _verbose: bool,
        _use_markdown: bool,
        agent_runner: Option<Arc<dyn ::runtime::api::core::tool::AgentRunner>>,
        allow_all: bool,
        interrupted: Arc<AtomicBool>,
        task_store: Arc<::runtime::api::core::task::TaskStore>,
        max_tool_concurrency: usize,
        max_agent_concurrency: usize,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
    ) -> io::Result<()> {
        let read_files = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let session_reminders = self.cmd_exec.session_reminders.clone();
        let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(256);
        self.chat.is_processing = false;
        let active_cancel: Arc<std::sync::Mutex<Option<CancellationToken>>> =
            Arc::new(std::sync::Mutex::new(None));

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

            // Build spawn context refs for update()
            let hook_runner_clone = self.cmd_exec.hook_runner.clone();
            let memory_config_clone = self.session.memory_config.clone();
            let json_logger_clone = self.cmd_exec.json_logger.clone();
            let spawn_refs = processing::SpawnContextRefs {
                client: &client,
                registry: &registry,
                system_blocks: &system_blocks,
                system_prompt_text: &system_prompt_text,
                user_context: &user_context,
                context_size,
                read_files: &read_files,
                session_reminders: &session_reminders,
                agent_runner: &agent_runner,
                allow_all,
                interrupted: &interrupted,
                task_store: &task_store,
                max_tool_concurrency,
                max_agent_concurrency,
                agent_semaphore: &agent_semaphore,
                hook_runner: &hook_runner_clone,
                memory_config: &memory_config_clone,
                json_logger: &json_logger_clone,
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
            let result = self.update(msg, &ui_tx, &active_cancel, &spawn_refs);

            // --- Handle pending slash commands (async) ---
            if let Some(input) = result.pending_slash {
                let review_prompt = self
                    .handle_slash_command_with_events(&input, Some(ui_tx.clone()))
                    .await;
                if let Some(prompt) = review_prompt {
                    self.output_area.push_user_message(&input);
                    self.chat.messages
                        .push(::runtime::api::core::message::Message::user(&prompt));
                    interrupted.store(false, Ordering::Relaxed);
                    self.output_area.start_spinner();
                    self.output_area.set_spinner_phase("Thinking...");
                    self.chat.is_processing = true;
                    let cancel = CancellationToken::new();
                    if let Ok(mut guard) = active_cancel.lock() {
                        *guard = Some(cancel.clone());
                    }
                    processing::spawn_processing(processing::SpawnContext {
                        tx: ui_tx.clone(),
                        queue_request_tx: ui_tx.clone(),
                        client: client.clone(),
                        registry: registry.clone(),
                        system_blocks: system_blocks.clone(),
                        system_prompt_text: system_prompt_text.clone(),
                        user_context: user_context.clone(),
                        messages: self.chat.messages.clone(),
                        context_size,
                        cwd: self.session.cwd.clone(),
                        workspace_context: self.cmd_exec.workspace_context.clone(),
                        session_id: self.session.session_id.clone(),
                        read_files: read_files.clone(),
                        session_reminders: self.cmd_exec.session_reminders.clone(),
                        agent_runner: agent_runner.clone(),
                        allow_all,
                        interrupted: interrupted.clone(),
                        cancel,
                        task_store: task_store.clone(),
                        max_tool_concurrency,
                        max_agent_concurrency,
                        agent_semaphore: agent_semaphore.clone(),
                        hook_runner: self.cmd_exec.hook_runner.clone(),
                        memory_config: self.session.memory_config.clone(),
                        json_logger: self.cmd_exec.json_logger.clone(),
                    });
                }
            }

            // --- TEA command execution ---
            match result.cmd {
                Cmd::None => {}
                Cmd::Quit => {
                    self.layout.should_exit = true;
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
                Cmd::QueueInput(_) => {
                    // Handled via pending_slash above
                }
                Cmd::SaveSession(msgs) => {
                    if !msgs.is_empty() {
                        let s = self.build_session(msgs).await;
                        if let Err(e) = ::runtime::api::core::session::save_session(&s).await {
                            log::warn!("failed to auto-save session on sync: {e}");
                        }
                    }
                }
                Cmd::RunHookNotification { message, kind } => {
                    let hook_runner = self.cmd_exec.hook_runner.clone();
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
                                    .send(UiEvent::SystemMessage(format!(
                                        "Failed to load image: {e}"
                                    )))
                                    .await;
                            }
                        }
                    });
                }
            }

            self.input.just_pasted = false;
            if self.layout.should_exit {
                break;
            }
        }
        Ok(())
    }
}
