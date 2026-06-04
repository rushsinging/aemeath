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
use crate::tui::adapter::live_status_widget::apply_live_status_to_widget;
use crate::tui::adapter::output_widget::render_document_from_view_model;
use crate::tui::adapter::status_widget::{
    apply_diagnostic_status_to_widget, apply_runtime_status_to_widget,
};
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::effect::session::processing::SpawnContext;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::update::msg::TuiMsg;
use crate::tui::update::root_reducer::{reduce_agent_event, TuiUpdateResult};
use crate::tui::view_assembler::output::OutputViewAssembler;
use tokio::sync::mpsc;

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

    pub fn spawn_processing(spawn_ctx: SpawnContext) -> Self {
        Self {
            effects: Vec::new(),
            spawn_effect: Some(SpawnAgentChatEffect {
                chat_id: "legacy-processing".to_string(),
                prompt: String::new(),
                context: Some(spawn_ctx),
            }),
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
            TuiMsg::Key(key) => self.update_key(key, ui_tx, spawn_refs),
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
                        self.append_system_notice("[reading clipboard image...]");
                        return UpdateResult::one(Effect::ReadClipboardImage);
                    }
                    sdk::PasteKind::ImageFile => {
                        self.append_system_notice(format!("[loading image: {}...]", text.trim()));
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
                self.view_state.spinner.advance();
                UpdateResult::none()
            }
            TuiMsg::TerminalKey(key) => self.update_key(key, ui_tx, spawn_refs),
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
        let model_result = if mapping == Default::default() {
            TuiUpdateResult::default()
        } else {
            reduce_agent_event(&mut self.model, mapping)
        };
        let mut result = self.update_ui(ev, ui_tx, spawn_refs);
        crate::tui::update::dirty::merge_dirty(&mut self.view_state.dirty, model_result.dirty);
        result.effects.extend(model_result.effects);
        result
    }

    pub(crate) fn refresh_output_widget_from_model(&mut self) {
        let view_model = OutputViewAssembler::assemble_from_conversation(
            &self.model.conversation,
            self.view_state.output.version,
        );
        let width = self.layout.output_area_rect.width.saturating_sub(3).max(1);
        render_document_from_view_model(&mut self.output_area, &view_model, width);
    }

    pub(crate) fn flush_dirty_view_models(&mut self) {
        if self.view_state.dirty.output {
            self.refresh_output_widget_from_model();
            self.view_state.dirty.clear_output();
        }
        if self.view_state.dirty.status {
            apply_runtime_status_to_widget(&self.model, &mut self.status_bar);
            apply_diagnostic_status_to_widget(&self.model, &mut self.status_bar);
            self.view_state.dirty.clear_status();
        }
    }

    pub(crate) fn mark_output_dirty(&mut self) {
        self.view_state.dirty.mark_output();
    }

    /// 据 Model 业务态（spinner.active + phase / task lines / queued submissions）
    /// + view_state 动画态（frame/verb）派生实时状态行，单向写回 widget 镜像。
    /// 这是 spinner/task/queued live-status 镜像的唯一写入路径。
    ///
    /// verb/active 检测属 effectful 边界（rng/激活检测），故放在此渲染前的副作用处，
    /// 而非纯 reducer：
    /// - 由 inactive→active（verb 为空）时一次性 `pick_verb`（选 verb + 复位 frame=0）；
    /// - inactive 时复位 view_state.spinner，使下次激活重新 pick，elapsed/frame 归零。
    pub(crate) fn refresh_live_status_from_model(&mut self) {
        let active = self.model.runtime.spinner.active;
        if active {
            if self.view_state.spinner.verb.is_empty() {
                self.view_state.spinner.pick_verb();
            }
        } else if self.view_state.spinner != crate::tui::view_state::SpinnerAnim::default() {
            self.view_state.spinner = crate::tui::view_state::SpinnerAnim::default();
        }
        let queued_texts: Vec<String> = self
            .model
            .conversation
            .queued_submissions
            .iter()
            .map(|q| q.text.clone())
            .collect();
        let vm = crate::tui::view_assembler::live_status::LiveStatusAssembler::assemble(
            &self.model.runtime,
            &self.view_state.spinner,
            &queued_texts,
        );
        apply_live_status_to_widget(&mut self.output_area, &vm);
    }

    /// 据 OutputViewState 滚动真相执行 last_visible_height 反喂、内容增长补偿与钳制。
    /// 每帧渲染前调用；OutputArea render 直接消费 view_state.output，不再写 widget 镜像。
    pub(crate) fn refresh_output_scroll_from_view_state(&mut self) {
        crate::tui::adapter::output_view_widget::sync_output_scroll_view_state(
            &mut self.view_state.output,
            &self.output_area,
        );
        // #70 phase 2：output selection/scroll render 直接消费 view_state.output，无 widget 镜像写回。
        // #70 phase 2：status 选区 render 直接消费 view_state.status_sel，无 widget 镜像写回。
        // #70 phase 2：input 选区 render 直接消费 view_state.input_sel，无 widget 镜像写回。
    }
}

/// Type alias so update.rs can use `App` without circular path
use super::App;
