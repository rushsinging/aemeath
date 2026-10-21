use super::UpdateResult;
use crate::tui::app::App;
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_dialog_key(app: &mut App, key: KeyEvent) -> Option<UpdateResult> {
    if !app.layout.has_active_dialog() {
        return None;
    }

    match key.code {
        KeyCode::Up => {
            if let Some(d) = app.layout.active_dialog_mut() {
                d.select_prev();
            }
        }
        KeyCode::Down => {
            if let Some(d) = app.layout.active_dialog_mut() {
                d.select_next();
            }
        }
        KeyCode::Enter => {
            if let Some(selected) = app.layout.selected_model_key() {
                let prefix = app
                    .layout
                    .dialog_command_prefix
                    .clone()
                    .unwrap_or_else(|| "/model".to_string());
                let command = format!("{prefix} {selected}");
                app.layout.clear_dialog();
                // 对话框确认等价于提交 `{prefix} {selection}`（/model、/think
                // 共用），走同一同步分发。
                return Some(app.handle_slash_command(&command));
            }
            app.layout.clear_dialog();
        }
        KeyCode::Esc => app.layout.clear_dialog(),
        _ => return None,
    }

    Some(UpdateResult::none())
}
