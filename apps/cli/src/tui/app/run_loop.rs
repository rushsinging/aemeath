use super::App;
use crate::tui::app::event::UiEvent;
use crate::tui::effect::session::processing;
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

    /// #390 A1：建立常驻 chat() 处理回路（启动一次 + `/clear` 后自愈重建）。
    ///
    /// 经 `build_spawn_context` 建一条新的 input_events 通道（sender 存入
    /// `chat.input_event_tx`，port 随 `ChatRequest.input_events` 传给 runtime），
    /// 以当前历史 `messages` 调一次 `chat()` 并 spawn 长生命周期流消费任务。
    /// 已存在通道（`input_event_tx` 为 Some）时为 no-op，调用安全幂等。
    fn ensure_persistent_processing(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        if self.chat.input_event_tx.is_some() {
            return;
        }
        let spawn_refs = processing::SpawnContextRefs {
            agent_client: self.agent_client.clone(),
        };
        match self.build_spawn_context(ui_tx, &spawn_refs) {
            Some(spawn_ctx) => {
                let handle = processing::spawn_processing(spawn_ctx);
                self.chat.set_processing_handle(handle);
            }
            None => self.append_error_notice("SDK agent client is unavailable"),
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

        // #390 A1：常驻 chat() 模型——启动时调一次 chat()，常驻 loop 顶部 idle-wait
        // 直到首条 UserMessage 经 input_events 通道到达；此后每次提交（首条 / 插话）
        // 都复用此通道，不再 per-submit spawn。messages 为当前历史（新会话为空，
        // resume 为已加载历史）。
        self.ensure_persistent_processing(&ui_tx);

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
                    // #390 A1：slash 命令产出的 LLM prompt（如 /review）改为经常驻
                    // input_events 通道发往 loop，不再 spawn 新 chat。回显由 runtime 的
                    // MessagesSync 单一真相驱动（与普通提交一致）。
                    interrupted.store(false, Ordering::Relaxed);
                    self.chat.clear_tool_activity();
                    self.spinner_phase(SpinnerPhase::Thinking);
                    self.chat.start_processing();
                    self.chat
                        .push_input_event(sdk::ChatInputEvent::UserMessage {
                            id: sdk::InputId::new_v7(),
                            text: prompt,
                            images: Vec::new(),
                        });
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

            // 防御性重建：常驻 loop 异常退出（panic / receiver dropped）时 input_event_tx
            // 变 None，此处检测并重建，使后续提交仍可经事件通道驱动。正常 /clear 不再
            // drop tx（#391 S2 已统一为 runtime gate 清 messages，loop 存活）。
            if !self.layout.should_exit && self.chat.input_event_tx.is_none() {
                self.ensure_persistent_processing(&ui_tx);
            }
        }

        // #390 A1 常驻 loop 退出收尾（覆盖所有退出路径：/exit、/quit、Ctrl+C 强退等）：
        // drop input_event_tx → 常驻 loop recv_next 收到 None（shutdown）→ loop 干净退出，
        // 不再 hang；再 abort 消费句柄兜底。clear_input_event_buffer 幂等，重复调用安全。
        self.chat.clear_input_event_buffer();
        self.chat.abort_processing_handle();

        Ok(())
    }
}
