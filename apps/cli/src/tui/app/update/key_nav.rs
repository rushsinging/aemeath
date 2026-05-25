use super::UpdateResult;
use crate::tui::app::msg::Cmd;
use crate::tui::app::App;
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_dialog_key(app: &mut App, key: KeyEvent) -> Option<UpdateResult> {
    if app.layout.active_dialog.is_none() {
        return None;
    }

    match key.code {
        KeyCode::Up => {
            if let Some(ref mut d) = app.layout.active_dialog {
                d.select_prev();
            }
        }
        KeyCode::Down => {
            if let Some(ref mut d) = app.layout.active_dialog {
                d.select_next();
            }
        }
        KeyCode::Enter => {
            let selected = app.layout.active_dialog.as_ref().and_then(|d| d.get_selected());
            if let Some(idx) = selected {
                if idx < app.layout.dialog_model_keys.len() {
                    let model_key = app.layout.dialog_model_keys[idx].clone();
                    app.input.input_queue.push_back(format!("/model {}", model_key));
                    app.layout.active_dialog = None;
                    app.layout.dialog_model_keys.clear();
                    return Some(UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: Some(format!("/model {}", model_key)),
                    });
                }
            }
            app.layout.active_dialog = None;
            app.layout.dialog_model_keys.clear();
        }
        KeyCode::Esc => {
            app.layout.active_dialog = None;
            app.layout.dialog_model_keys.clear();
        }
        _ => return None,
    }

    Some(UpdateResult {
        cmd: Cmd::None,
        pending_slash: None,
    })
}
