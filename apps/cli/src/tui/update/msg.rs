use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use crate::tui::app::event::UiEvent;
use crate::tui::effect::effect::EffectResult;
use crossterm::event::{KeyEvent, MouseEvent};

#[derive(Debug)]
pub enum TuiMsg {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize { width: u16, height: u16 },
    SpinnerTick,
    Ui(UiEvent),
    Runtime(TuiRuntimeEvent),
    TerminalKey(KeyEvent),
    TerminalMouse(MouseEvent),
    TerminalResize { width: u16, height: u16 },
    AgentEvent(UiEvent),
    EffectCompleted(EffectResult),
    TimerTick { id: String },
    RenderTick,
}
