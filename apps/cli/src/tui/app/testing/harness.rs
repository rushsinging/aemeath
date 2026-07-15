use std::collections::VecDeque;

use crossterm::event::KeyEvent;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::tui::app::App;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::update::msg::TuiMsg;

use super::effect_driver::RecordingEffectDriver;

pub(crate) struct TuiScenarioHarness {
    app: App,
    terminal: Terminal<TestBackend>,
    messages: VecDeque<TuiMsg>,
    effects: RecordingEffectDriver,
    ui_tx: mpsc::Sender<crate::tui::app::event::UiEvent>,
}

impl TuiScenarioHarness {
    pub fn new(width: u16, height: u16) -> Self {
        let (ui_tx, _ui_rx) = mpsc::channel(16);
        Self {
            app: super::fixture::app(),
            terminal: Terminal::new(TestBackend::new(width, height)).expect("test terminal"),
            messages: VecDeque::new(),
            effects: RecordingEffectDriver::default(),
            ui_tx,
        }
    }

    pub fn key(&mut self, event: KeyEvent) {
        self.messages.push_back(TuiMsg::Key(event));
        self.drain(32);
    }

    pub fn drain(&mut self, max_steps: usize) {
        let mut steps = 0;
        while let Some(message) = self.messages.pop_front() {
            assert!(steps < max_steps, "scenario exceeded {max_steps} steps");
            let outcome = self.app.drive_frame(
                message,
                &self.ui_tx,
                &SpawnContextRefs { agent_client: None },
            );
            self.effects.record(outcome);
            steps += 1;
        }
    }

    pub fn render(&mut self) {
        self.app.prepare_frame();
        self.app.draw(&mut self.terminal).expect("TestBackend draw");
    }

    pub fn screen(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn input_text(&self) -> String {
        self.app.model.input.document.buffer.clone()
    }

    pub fn assert_idle(&self) {
        assert!(self.messages.is_empty(), "pending messages remain");
        assert!(self.effects.is_idle(), "pending spawn/slash effects remain");
    }
}
