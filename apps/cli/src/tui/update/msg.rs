use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use crate::tui::app::event::UiEvent;
use crossterm::event::{KeyEvent, MouseEvent};

#[derive(Debug)]
pub enum TuiMsg {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize { width: u16, height: u16 },
    SpinnerTick,
    Ui(UiEvent),
    RuntimeBatch(Vec<TuiRuntimeEvent>),
}
