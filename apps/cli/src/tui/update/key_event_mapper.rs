use crate::tui::model::input::intent::InputIntent;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyEventMapping {
    pub input: Vec<InputIntent>,
    pub submit_requested: bool,
    pub quit_requested: bool,
}

pub fn map_key_event(key: KeyEvent) -> KeyEventMapping {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyEventMapping {
            quit_requested: true,
            ..KeyEventMapping::default()
        },
        KeyCode::Char(ch) => input(InputIntent::InsertChar(ch)),
        KeyCode::Enter => KeyEventMapping {
            submit_requested: true,
            ..KeyEventMapping::default()
        },
        KeyCode::Backspace => input(InputIntent::DeleteBackward),
        KeyCode::Delete => input(InputIntent::DeleteForward),
        KeyCode::Left => input(InputIntent::MoveCursorLeft),
        KeyCode::Right => input(InputIntent::MoveCursorRight),
        KeyCode::Home => input(InputIntent::MoveCursorHome),
        KeyCode::End => input(InputIntent::MoveCursorEnd),
        KeyCode::Up => input(InputIntent::MoveHistoryPrevious),
        KeyCode::Down => input(InputIntent::MoveHistoryNext),
        KeyCode::Esc => input(InputIntent::Clear),
        _ => KeyEventMapping::default(),
    }
}

fn input(intent: InputIntent) -> KeyEventMapping {
    KeyEventMapping {
        input: vec![intent],
        ..KeyEventMapping::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_map_key_event_char_to_insert() {
        let mapping = map_key_event(key(KeyCode::Char('a')));
        assert!(matches!(
            mapping.input.first(),
            Some(InputIntent::InsertChar('a'))
        ));
    }

    #[test]
    fn test_map_key_event_enter_requests_submit() {
        let mapping = map_key_event(key(KeyCode::Enter));
        assert!(mapping.submit_requested);
    }

    #[test]
    fn test_map_key_event_ctrl_c_requests_quit() {
        let mut event = key(KeyCode::Char('c'));
        event.modifiers = KeyModifiers::CONTROL;
        let mapping = map_key_event(event);
        assert!(mapping.quit_requested);
    }
}
