mod ask_user_key;
mod ask_user_options;
mod done;
mod enter;
mod key;
mod key_nav;
mod key_scroll;
mod reminder;
mod spawn_context;
mod spinner;
mod ui_event;

pub(crate) use key::CTRL_C_TIMEOUT_SECS;

use super::event::UiEvent;
use crate::tui::adapter::agent_event::map_agent_event;
use crate::tui::adapter::output_widget::replace_lines_from_view_model;
use crate::tui::adapter::status_widget::{
    apply_diagnostic_status_to_widget, apply_runtime_status_to_widget,
};
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::render::output_view_model::output_view_model_lines;
use crate::tui::session::processing::SpawnContext;
use crate::tui::session::processing::SpawnContextRefs;
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
                self.handle_mouse_event(mouse, self.layout.output_area_rect);
                UpdateResult::none()
            }
            TuiMsg::Paste(text) if !self.chat.is_processing => {
                self.handle_paste_event(text, ui_tx);
                UpdateResult::none()
            }
            TuiMsg::Paste(text) => {
                // Paste while in AskUserQuestion free-input mode: insert into input area only
                if self.input.ask_user_state.is_some() || self.input.ask_user_reply_tx.is_some() {
                    self.input.just_pasted = true;
                    for ch in text.chars() {
                        if ch == '\n' || ch == '\r' {
                            self.input_area.enter(true);
                        } else {
                            self.input_area.input(ch);
                        }
                    }
                    // 同步模型状态：paste 直接修改 textarea 未走模型（同 #77/#78）
                    let text = self.input_area.get_text();
                    self.model.input.document.clear();
                    self.model.input.document.insert_text(&text);
                    return UpdateResult::none();
                }
                // Paste while processing: insert into input area so it can be queued
                match sdk::classify_paste(&text) {
                    sdk::PasteKind::Empty => {
                        self.input.just_pasted = true;
                        self.output_area.push_system("[reading clipboard image...]");
                        return UpdateResult::one(Effect::ReadClipboardImage);
                    }
                    sdk::PasteKind::ImageFile => {
                        self.output_area
                            .push_system(&format!("[loading image: {}...]", text.trim()));
                        self.input.just_pasted = true;
                        return UpdateResult::one(Effect::ProcessImageFile {
                            path: text.trim().to_string(),
                        });
                    }
                    sdk::PasteKind::Text => {
                        self.input.just_pasted = true;
                        for ch in text.chars() {
                            if ch == '\n' || ch == '\r' {
                                self.input_area.enter(true);
                            } else {
                                self.input_area.input(ch);
                            }
                        }
                        // 同步模型状态：paste 直接修改 textarea 未走模型（同 #77/#78）
                        let text = self.input_area.get_text();
                        self.model.input.document.clear();
                        self.model.input.document.insert_text(&text);
                    }
                }
                UpdateResult::none()
            }
            TuiMsg::Resize { width, height } => {
                self.handle_resize(width, height);
                UpdateResult::none()
            }
            TuiMsg::SpinnerTick => {
                self.output_area.tick_spinner();
                UpdateResult::none()
            }
            TuiMsg::TerminalKey(key) => self.update_key(key, ui_tx, spawn_refs),
            TuiMsg::TerminalMouse(mouse) => {
                self.handle_mouse_event(mouse, self.layout.output_area_rect);
                UpdateResult::none()
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
        self.refresh_output_widget_from_model();
        apply_runtime_status_to_widget(
            &self.model,
            self.chat.last_input_tokens,
            &mut self.status_bar,
        );
        apply_diagnostic_status_to_widget(&self.model, &mut self.status_bar);
        result.effects.extend(model_result.effects);
        result
    }

    fn refresh_output_widget_from_model(&mut self) {
        let view_model = OutputViewAssembler::assemble_from_conversation(
            &self.model.conversation,
            self.view_state.output.version,
        );
        let lines = output_view_model_lines(&view_model);
        if !lines.is_empty() {
            replace_lines_from_view_model(&mut self.output_area, lines);
        }
    }
}

/// Type alias so update.rs can use `App` without circular path
use super::App;
