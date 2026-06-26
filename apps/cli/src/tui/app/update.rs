mod ask_user_key;
mod done;
mod enter;
mod key;
mod key_nav;
mod key_scroll;
mod notice;
mod reminder;
mod spawn_context;
mod spinner;
mod ui_event;

pub(crate) use key::CTRL_C_TIMEOUT_SECS;

use super::event::UiEvent;
use crate::tui::adapter::agent_event::map_agent_event;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crate::tui::render::output_area::SCROLLBAR_RESERVE_COLS;
use crate::tui::update::msg::TuiMsg;
use crate::tui::update::root_reducer::{reduce_agent_event, TuiUpdateResult};
use crate::tui::view_assembler::output::OutputViewAssembler;
use crate::tui::view_model::LiveStatusViewModel;
use tokio::sync::mpsc;

fn ui_event_name(event: &UiEvent) -> &'static str {
    match event {
        UiEvent::Text { .. } => "Text",
        UiEvent::Thinking { .. } => "Thinking",
        UiEvent::BlockComplete { .. } => "BlockComplete",
        UiEvent::ToolCallStart { .. } => "ToolCallStart",
        UiEvent::ToolCallUpdate { .. } => "ToolCallUpdate",
        UiEvent::ToolResult { .. } => "ToolResult",
        UiEvent::Usage { .. } => "Usage",
        UiEvent::Error(_) => "Error",
        UiEvent::Cancelled { .. } => "Cancelled",
        UiEvent::MessagesSync(_) => "MessagesSync",
        UiEvent::UserMessagesAdded(_) => "UserMessagesAdded",
        UiEvent::Done { .. } => "Done",
        UiEvent::DoneWithDuration { .. } => "DoneWithDuration",
        UiEvent::LiveTps(_) => "LiveTps",
        UiEvent::ClipboardImage(_) => "ClipboardImage",
        UiEvent::SystemMessage(_) => "SystemMessage",
        UiEvent::ReminderRecap(_) => "ReminderRecap",
        UiEvent::MemoryList(_) => "MemoryList",
        UiEvent::SessionSaved { .. } => "SessionSaved",
        UiEvent::SlashCommandFailed { .. } => "SlashCommandFailed",
        UiEvent::ReflectionStarted => "ReflectionStarted",
        UiEvent::ReflectionUsage => "ReflectionUsage",
        UiEvent::ReflectionDone { .. } => "ReflectionDone",
        UiEvent::ReflectionApplyDone { .. } => "ReflectionApplyDone",
        UiEvent::AskUserBatch { .. } => "AskUserBatch",
        UiEvent::HookEvent(_) => "HookEvent",
        UiEvent::AgentProgress { .. } => "AgentProgress",
        UiEvent::WorkingDirectoryChanged { .. } => "WorkingDirectoryChanged",
        UiEvent::TaskStatusChanged => "TaskStatusChanged",
        UiEvent::CurrentTurnChanged(_) => "CurrentTurnChanged",
        UiEvent::UpdateAvailable { .. } => "UpdateAvailable",
        UiEvent::SessionReset => "SessionReset",
        UiEvent::UserMessagesWithdrawn(_) => "UserMessagesWithdrawn",
        UiEvent::GraphPhaseChanged { .. } => "GraphPhaseChanged",
        UiEvent::CompactProgress { .. } => "CompactProgress",
    }
}

pub(crate) fn output_visible_height(area_height: u16, live_status: &LiveStatusViewModel) -> usize {
    let spinner_line_count = usize::from(live_status.spinner.is_some());
    let task_line_count = if live_status.spinner.is_some() {
        live_status.task_lines.len()
    } else {
        0
    };
    // No-spinner path reserves exactly queued line count; empty queue naturally reserves 0.
    let reserved = if live_status.spinner.is_some() {
        live_status.queued_lines.len() + spinner_line_count + task_line_count
    } else {
        live_status.queued_lines.len()
    };
    (area_height as usize).saturating_sub(reserved)
}

/// Return type for update: effects plus optional slash command continuation.
pub struct UpdateResult {
    pub effects: Vec<Effect>,
    pub spawn_effect: Option<SpawnAgentChatEffect>,
    pub pending_slash: Option<String>,
}

impl UpdateResult {
    pub fn none() -> Self {
        Self {
            effects: Vec::new(),
            spawn_effect: None,
            pending_slash: None,
        }
    }

    pub fn one(effect: Effect) -> Self {
        Self {
            effects: vec![effect],
            spawn_effect: None,
            pending_slash: None,
        }
    }
}

impl App {
    /// TEA-style update: pure state transition based on a message.
    /// Returns commands for the runtime to execute.
    pub(crate) fn update(
        &mut self,
        msg: TuiMsg,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        match msg {
            TuiMsg::Ui(ev) => self.update_agent_event(ev, ui_tx, spawn_refs),
            TuiMsg::AgentEvent(ev) => self.update_agent_event(ev, ui_tx, spawn_refs),
            TuiMsg::Key(key) => self.update_key(key, spawn_refs),
            TuiMsg::Mouse(mouse) => {
                let effects = self.handle_mouse_event(mouse, self.layout.output_area_rect);
                UpdateResult {
                    effects,
                    spawn_effect: None,
                    pending_slash: None,
                }
            }
            TuiMsg::Paste(text) if !self.chat.is_processing => {
                self.handle_paste_event(text, ui_tx);
                UpdateResult::none()
            }
            TuiMsg::Paste(text) => {
                // Paste while in AskUserQuestion free-input mode: insert into input area only
                if self.input.ask_user_state.is_some() || self.input.ask_user_reply_tx.is_some() {
                    self.input.just_pasted = true;
                    self.handle_input_intent(
                        crate::tui::model::input::intent::InputIntent::InsertText(text),
                    );
                    return UpdateResult::none();
                } // Paste while processing: insert into input area so it can be queued
                match sdk::classify_paste(&text) {
                    sdk::PasteKind::Empty => {
                        self.input.just_pasted = true;
                        // 删：[reading clipboard image...] —— 同 paste_handler.rs 路径（#fix-tui-image-input-output）
                        return UpdateResult::one(Effect::ReadClipboardImage);
                    }
                    sdk::PasteKind::ImageFile => {
                        // 删：[loading image: ...] —— 同上（#fix-tui-image-input-output）
                        self.input.just_pasted = true;
                        return UpdateResult::one(Effect::ProcessImageFile {
                            path: text.trim().to_string(),
                        });
                    }
                    sdk::PasteKind::Text => {
                        self.input.just_pasted = true;
                        self.handle_input_intent(
                            crate::tui::model::input::intent::InputIntent::InsertText(text),
                        );
                    }
                }
                UpdateResult::none()
            }
            TuiMsg::Resize { width, height } => {
                self.handle_resize(width, height);
                UpdateResult::none()
            }
            TuiMsg::SpinnerTick => {
                // 动画帧真相归 view_state；spinner 是否可见由 Model 决定，
                // 镜像写回统一在每帧渲染前的 refresh_live_status_from_model。
                let before_frame = self.view_state.animation.spinner_frame;
                let before_version = self.view_state.animation.version;
                self.view_state.animation.spinner_frame =
                    self.view_state.animation.spinner_frame.wrapping_add(1);
                self.view_state.animation.version =
                    self.view_state.animation.version.wrapping_add(1);
                self.view_state.spinner.advance();
                // 临时 status notice 过期检查：到期回退到 graph_phase 派生态。
                if self
                    .model
                    .runtime
                    .expire_transient_notice(std::time::Instant::now())
                {
                    self.mark_output_dirty();
                }
                // 仅在处理中（有运行中 block 的 gutter 动画需要重绘）时才标脏 output。
                // idle/完成态标脏会导致每 90ms 全量重建整会话 → 大会话伪卡死（live-lock）。
                if self.model.runtime.spinner.active {
                    self.mark_output_dirty();
                }
                crate::tui::log_trace!(
                    "tui.spinner.tick before_frame={} after_frame={} before_version={} after_version={} anim_frame={} active={} phase={:?} verb={} dirty_output={}",
                    before_frame,
                    self.view_state.animation.spinner_frame,
                    before_version,
                    self.view_state.animation.version,
                    self.view_state.spinner.frame,
                    self.model.runtime.spinner.active,
                    self.model.runtime.spinner.phase,
                    self.view_state.spinner.verb,
                    self.view_state.dirty.output
                );
                UpdateResult::none()
            }
            TuiMsg::TerminalKey(key) => self.update_key(key, spawn_refs),
            TuiMsg::TerminalMouse(mouse) => {
                let effects = self.handle_mouse_event(mouse, self.layout.output_area_rect);
                UpdateResult {
                    effects,
                    spawn_effect: None,
                    pending_slash: None,
                }
            }
            TuiMsg::TerminalResize { width, height } => {
                self.handle_resize(width, height);
                UpdateResult::none()
            }
            TuiMsg::EffectCompleted(_) | TuiMsg::TimerTick { .. } | TuiMsg::RenderTick => {
                UpdateResult::none()
            }
        }
    }

    fn update_agent_event(
        &mut self,
        ev: UiEvent,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        let mapping = map_agent_event(&ev);
        crate::tui::log_trace!(
            "tui.agent_event mapped event={} conversation_intents={} runtime_intents={} diagnostic_intents={} session_intents={} effects={}",
            ui_event_name(&ev),
            mapping.conversation.len(),
            mapping.runtime.len(),
            mapping.diagnostic.len(),
            mapping.session.len(),
            mapping.effects.len()
        );
        let model_result = if mapping == Default::default() {
            TuiUpdateResult::default()
        } else {
            reduce_agent_event(&mut self.model, mapping)
        };
        crate::tui::log_trace!(
            "tui.agent_event reduced event={} dirty_output={} dirty_status={} dirty_dialog={} dirty_input={} effects={} timeline_items={} chats={}",
            ui_event_name(&ev),
            model_result.dirty.output,
            model_result.dirty.status,
            model_result.dirty.dialog,
            model_result.dirty.input,
            model_result.effects.len(),
            self.model.conversation.timeline.items().len(),
            self.model.conversation.chats.len()
        );
        let mut result = self.update_ui(ev, ui_tx, spawn_refs);
        crate::tui::update::dirty::merge_dirty(&mut self.view_state.dirty, model_result.dirty);
        result.effects.extend(model_result.effects);
        result
    }

    pub(crate) fn output_document_width(&self) -> u16 {
        self.layout
            .output_area_rect
            .width
            .saturating_sub(SCROLLBAR_RESERVE_COLS)
            .max(1)
    }

    pub(crate) fn refresh_output_document_from_model(&mut self) {
        let before_lines = self.output_area.document().total_lines();
        let revision = self.model.conversation.revision();
        let current_workspace_root: Option<String> = self
            .model
            .runtime
            .workspace
            .workspace_root
            .as_deref()
            .map(|s| s.to_owned());
        // memo：conversation revision 不变 且 workspace_root 不变 则复用上次 view_model，跳过全量 assemble。
        // workspace_root 来自 /worktree enter，不推进 revision，需单独纳入 key（#425 review Fix 1）。
        let need_rebuild = self
            .output_view_cache
            .as_ref()
            .map(|cache| {
                cache.revision != revision
                    || cache.workspace_root.as_deref() != current_workspace_root.as_deref()
            })
            .unwrap_or(true);
        if need_rebuild {
            #[cfg(test)]
            {
                self.assemble_count += 1;
            }
            let workspace_root = current_workspace_root.as_deref().map(std::path::Path::new);
            let view_model = OutputViewAssembler::assemble_from_conversation(
                &self.model.conversation,
                revision,
                workspace_root,
            );
            self.output_view_cache = Some(OutputViewCache {
                revision,
                workspace_root: current_workspace_root.clone(),
                view_model,
            });
        }
        // take 出 owned view_model，render 期间释放对 cache 的不可变借用，render 后放回。
        let cache = self
            .output_view_cache
            .take()
            .expect("memo cache filled above");
        let view_model = cache.view_model;
        let cached_revision = cache.revision;
        let cached_workspace_root = cache.workspace_root;
        let root_count = view_model.roots.len();
        let width = self.output_document_width();
        // 文档构建（含各 block 的字符串处理）放在 draw 之外，draw 循环的 catch_unwind
        // 只保护「把已构建文档画进 buffer」，无法兜住这里的 panic。对称地在构建侧兜底：
        // 一旦构建 panic（已落 panic.log），保留旧文档并提示用户，避免崩溃与糊屏。
        let render_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.output_document_renderer.render_model_document(
                &view_model,
                width,
                self.output_area.term_width,
                self.view_state.animation.spinner_frame,
            )
        }));
        // 无论渲染成败都把 view_model 放回 cache，保留 memo。
        self.output_view_cache = Some(OutputViewCache {
            revision: cached_revision,
            workspace_root: cached_workspace_root,
            view_model,
        });
        let document =
            match render_result {
                Ok(document) => document,
                Err(_) => {
                    crate::tui::log_warn!(
                        "tui.output.refresh_document panicked; keeping previous document"
                    );
                    self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
                        StatusNotice::warning("渲染失败，已记录 panic.log"),
                    ));
                    return;
                }
            };
        let after_lines = document.total_lines();
        crate::tui::log_trace!(
            "tui.output.refresh_document revision={} width={} term_width={} spinner_frame={} roots={} timeline_items={} chats={} before_lines={} after_lines={} rebuilt={}",
            revision,
            width,
            self.output_area.term_width,
            self.view_state.animation.spinner_frame,
            root_count,
            self.model.conversation.timeline.items().len(),
            self.model.conversation.chats.len(),
            before_lines,
            after_lines,
            need_rebuild
        );
        self.output_area.replace_document(document);
    }
    pub(crate) fn flush_dirty_view_models(&mut self) {
        if self.view_state.dirty.output {
            self.refresh_output_document_from_model();
            self.view_state.dirty.clear_output();
        }
        if self.view_state.dirty.status {
            self.view_state.dirty.clear_status();
        }
    }
    pub(crate) fn mark_output_dirty(&mut self) {
        self.view_state.dirty.mark_output();
    }

    pub(crate) fn status_view_model(&self) -> crate::tui::view_model::StatusViewModel {
        crate::tui::view_assembler::status::StatusViewAssembler::assemble_status_view(
            &self.model.runtime,
            Some(&self.model.session),
            &self.model.diagnostic,
        )
    }

    pub(crate) fn dialog_view_model(&self) -> Option<crate::tui::view_model::DialogViewModel> {
        crate::tui::view_assembler::dialog::DialogViewAssembler::assemble_from_diagnostic(
            &self.model.diagnostic,
        )
    }

    /// 据 Model 业务态（spinner.active + phase / task lines / queued submissions）
    /// + view_state 动画态（frame/verb）派生实时状态行 ViewModel。
    pub(crate) fn live_status_view_model(&self) -> crate::tui::view_model::LiveStatusViewModel {
        let queued_texts: Vec<String> = self
            .model
            .conversation
            .queued_submissions
            .iter()
            .map(|q| q.text.clone())
            .collect();
        crate::tui::view_assembler::live_status::LiveStatusAssembler::assemble(
            &self.model.runtime,
            &self.view_state.spinner,
            &queued_texts,
        )
    }

    /// 渲染前维护 live-status 相关 view_state：
    /// - active 且 verb 为空时选择动词；
    /// - active 时同步 phase，phase 变化只重置 phase 计时；
    /// - inactive 时清空动画状态，保证下次激活重新计时。
    ///
    /// OutputArea render 直接消费 `live_status_view_model()`，不再写 widget mirror。
    ///
    /// verb/active 检测属 effectful 边界（rng/激活检测），故放在此渲染前的副作用处，
    /// 而非纯 reducer。
    pub(crate) fn refresh_live_status_from_model(&mut self) {
        let active = self.model.runtime.spinner.active;
        let before_anim = self.view_state.spinner.clone();
        if active {
            if self.view_state.spinner.verb.is_empty() {
                self.view_state.spinner.pick_verb();
            }
            self.view_state
                .spinner
                .sync_phase(self.model.runtime.spinner.phase.clone());
        } else if self.view_state.spinner != crate::tui::view_state::SpinnerAnim::default() {
            self.view_state.spinner = crate::tui::view_state::SpinnerAnim::default();
        }
        crate::tui::log_trace!(
            "tui.spinner.refresh active={} phase={:?} before_frame={} after_frame={} before_phase_frame={} after_phase_frame={} before_phase={:?} after_phase={:?} before_verb={} after_verb={}",
            active,
            self.model.runtime.spinner.phase,
            before_anim.frame,
            self.view_state.spinner.frame,
            before_anim.phase_frame,
            self.view_state.spinner.phase_frame,
            before_anim.phase,
            self.view_state.spinner.phase,
            before_anim.verb,
            self.view_state.spinner.verb
        );
    }
    /// 根据当前 document 与 layout/live-status 投影同步 OutputViewState 滚动真相。
    /// 每帧渲染前调用；OutputArea render 直接消费 view_state.output，不再写 widget 镜像。
    pub(crate) fn refresh_output_scroll_from_view_state(&mut self) {
        let visible_height = output_visible_height(
            self.layout.output_area_rect.height,
            &self.live_status_view_model(),
        );
        self.view_state
            .output
            .sync_document_metrics(self.output_area.document().total_lines(), visible_height);
        // #70 phase 2：output selection/scroll render 直接消费 view_state.output，无 widget 镜像写回。
        // #70 phase 2：status 选区 render 直接消费 view_state.status_sel，无 widget 镜像写回。
        // #70 phase 2：input 选区 render 直接消费 view_state.input_sel，无 widget 镜像写回。
    }
}

/// Type alias so update.rs can use `App` without circular path
use super::App;
use super::OutputViewCache;
