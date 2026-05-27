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
use super::msg::{Cmd, Msg};
use crate::tui::session::processing::SpawnContextRefs;
use tokio::sync::mpsc;

/// Return type for update: (commands, whether to continue the loop)
pub struct UpdateResult {
    pub cmd: Cmd,
    pub pending_slash: Option<String>,
}

impl App {
    /// TEA-style update: pure state transition based on a message.
    /// Returns commands for the runtime to execute.
    pub(crate) fn update(
        &mut self,
        msg: Msg,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        match msg {
            Msg::Key(key) => self.update_key(key, ui_tx, spawn_refs),
            Msg::Mouse(mouse) => {
                self.handle_mouse_event(mouse, self.layout.output_area_rect);
                UpdateResult {
                    cmd: Cmd::None,
                    pending_slash: None,
                }
            }
            Msg::Paste(text) if !self.chat.is_processing => {
                self.handle_paste_event(text, ui_tx);
                UpdateResult {
                    cmd: Cmd::None,
                    pending_slash: None,
                }
            }
            Msg::Paste(text) => {
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
                    return UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: None,
                    };
                }
                // Paste while processing: insert into input area so it can be queued
                match sdk::classify_paste(&text) {
                    sdk::PasteKind::Empty => {
                        self.input.just_pasted = true;
                        self.output_area.push_system("[reading clipboard image...]");
                        return UpdateResult {
                            cmd: Cmd::ReadClipboardImage,
                            pending_slash: None,
                        };
                    }
                    sdk::PasteKind::ImageFile => {
                        self.output_area
                            .push_system(&format!("[loading image: {}...]", text.trim()));
                        self.input.just_pasted = true;
                        return UpdateResult {
                            cmd: Cmd::ProcessImageFile(text.trim().to_string()),
                            pending_slash: None,
                        };
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
                UpdateResult {
                    cmd: Cmd::None,
                    pending_slash: None,
                }
            }
            Msg::Resize { width, height } => {
                self.handle_resize(width, height);
                UpdateResult {
                    cmd: Cmd::None,
                    pending_slash: None,
                }
            }
            Msg::SpinnerTick => {
                self.output_area.tick_spinner();
                UpdateResult {
                    cmd: Cmd::None,
                    pending_slash: None,
                }
            }
            Msg::Ui(ev) => self.update_ui(ev, ui_tx, spawn_refs),
        }
    }
}

/// Type alias so update.rs can use `App` without circular path
use super::App;
