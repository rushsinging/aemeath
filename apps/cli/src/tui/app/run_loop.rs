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
use std::time::Instant;
use tokio::sync::mpsc;

pub(crate) fn tui_msg_name(msg: &TuiMsg) -> &'static str {
    match msg {
        TuiMsg::Key(_) => "Key",
        TuiMsg::Mouse(_) => "Mouse",
        TuiMsg::Paste(_) => "Paste",
        TuiMsg::Resize { .. } => "Resize",
        TuiMsg::SpinnerTick => "SpinnerTick",
        TuiMsg::Ui(_) => "Ui",
        TuiMsg::TerminalKey(_) => "TerminalKey",
        TuiMsg::TerminalMouse(_) => "TerminalMouse",
        TuiMsg::TerminalResize { .. } => "TerminalResize",
        TuiMsg::AgentEvent(_) => "AgentEvent",
        TuiMsg::EffectCompleted(_) => "EffectCompleted",
        TuiMsg::TimerTick { .. } => "TimerTick",
        TuiMsg::RenderTick => "RenderTick",
    }
}

impl App {
    async fn handle_change_set(&mut self, change: ChangeSet) {
        crate::tui::log_trace!(
            "tui.change_set received bits={:?} contains_tasks={} contains_project={} contains_session={} contains_cost={}",
            change,
            change.contains(ChangeSet::TASKS),
            change.contains(ChangeSet::PROJECT),
            change.contains(ChangeSet::SESSION),
            change.contains(ChangeSet::COST)
        );
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

        // 启动时后台检查版本更新（非阻塞，失败静默降级）。
        self.spawn_update_check(ui_tx.clone());

        let mut event_stream = EventStream::new();
        let mut change_rx = self.agent_client.as_ref().map(|client| client.changes());
        let mut spinner_ticker = tokio::time::interval(std::time::Duration::from_millis(90));
        spinner_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_loop_iteration = Instant::now();

        // 首帧渲染先建立 layout 尺寸，再按真实宽度刷新启动横幅 document。
        self.update_task_status(self.chat.is_processing).await;
        self.update_project_context().await;
        self.draw(terminal)?;
        self.refresh_output_document_from_model();

        loop {
            let loop_now = Instant::now();
            let loop_gap_ms = loop_now.duration_since(last_loop_iteration).as_millis();
            last_loop_iteration = loop_now;
            crate::tui::log_trace!(
                "tui.loop.frame_begin gap_ms={} dirty_output={} dirty_status={} dirty_input={} dirty_dialog={} spinner_active={} spinner_phase={:?} spinner_frame={} output_lines={}",
                loop_gap_ms,
                self.view_state.dirty.output,
                self.view_state.dirty.status,
                self.view_state.dirty.input,
                self.view_state.dirty.dialog,
                self.model.runtime.spinner.active,
                self.model.runtime.spinner.phase,
                self.view_state.animation.spinner_frame,
                self.output_area.document().total_lines()
            );

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
                crate::tui::log_trace!("tui.loop.event msg=None");
                self.input.just_pasted = false;
                continue;
            };

            crate::tui::log_trace!(
                "tui.loop.event msg={} spinner_active={} spinner_phase={:?} spinner_frame={}",
                tui_msg_name(&msg),
                self.model.runtime.spinner.active,
                self.model.runtime.spinner.phase,
                self.view_state.animation.spinner_frame
            );

            // --- TEA update: state transition ---
            let update_start = Instant::now();
            let result = self.update(msg, &ui_tx, &spawn_refs);
            crate::tui::log_trace!(
                "tui.loop.update_complete elapsed_ms={} effects={} has_spawn_effect={} has_pending_slash={} dirty_output={} dirty_status={} dirty_input={} dirty_dialog={} spinner_active={} spinner_phase={:?} spinner_frame={}",
                update_start.elapsed().as_millis(),
                result.effects.len(),
                result.spawn_effect.is_some(),
                result.pending_slash.is_some(),
                self.view_state.dirty.output,
                self.view_state.dirty.status,
                self.view_state.dirty.input,
                self.view_state.dirty.dialog,
                self.model.runtime.spinner.active,
                self.model.runtime.spinner.phase,
                self.view_state.animation.spinner_frame
            );

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
