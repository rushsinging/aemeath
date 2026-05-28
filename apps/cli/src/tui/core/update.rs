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
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::session::processing::SpawnContext;
use crate::tui::session::processing::SpawnContextRefs;
use crate::tui::update::msg::TuiMsg;
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
            TuiMsg::Ui(ev) => self.update_ui(ev, ui_tx, spawn_refs),
            TuiMsg::TerminalKey(key) => self.update_key(key, ui_tx, spawn_refs),
            TuiMsg::TerminalMouse(mouse) => {
                self.handle_mouse_event(mouse, self.layout.output_area_rect);
                UpdateResult::none()
            }
            TuiMsg::TerminalResize { width, height } => {
                self.handle_resize(width, height);
                UpdateResult::none()
            }
            TuiMsg::AgentEvent(ev) => self.update_ui(ev, ui_tx, spawn_refs),
            TuiMsg::EffectCompleted(_) | TuiMsg::TimerTick { .. } | TuiMsg::RenderTick => {
                UpdateResult::none()
            }
        }
    }
}

/// Type alias so update.rs can use `App` without circular path
use super::App;
