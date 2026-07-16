use std::collections::VecDeque;

use crossterm::event::KeyEvent;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::tui::app::event::UiEvent;
use crate::tui::app::App;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::update::msg::TuiMsg;

use super::effect_driver::{ExpectedEffect, ScriptedEffectDriver};

pub(crate) struct TuiScenarioHarness {
    pub(crate) app: App,
    terminal: Terminal<TestBackend>,
    messages: VecDeque<TuiMsg>,
    effects: ScriptedEffectDriver,
    ui_tx: mpsc::Sender<UiEvent>,
    ticks: u64,
}

impl TuiScenarioHarness {
    pub fn new(width: u16, height: u16) -> Self {
        let (ui_tx, _ui_rx) = mpsc::channel(16);
        let mut harness = Self {
            app: super::fixture::app(),
            terminal: Terminal::new(TestBackend::new(width, height)).expect("test terminal"),
            messages: VecDeque::new(),
            effects: ScriptedEffectDriver::default(),
            ui_tx,
            ticks: 0,
        };
        harness.render();
        harness
    }

    pub fn expect_effect(&mut self, expected: ExpectedEffect) {
        self.effects.expect(expected);
    }
    pub fn key(&mut self, event: KeyEvent) {
        self.messages.push_back(TuiMsg::Key(event));
        self.drain(32);
    }
    pub fn ui(&mut self, event: UiEvent) {
        self.messages.push_back(TuiMsg::Ui(event));
        self.drain(32);
    }
    pub fn runtime(&mut self, event: UiEvent) {
        self.messages.push_back(TuiMsg::AgentEvent(event));
        self.drain(32);
    }
    pub fn tick(&mut self) {
        self.ticks += 1;
        self.messages.push_back(TuiMsg::SpinnerTick);
        self.drain(32);
    }

    pub fn step(&mut self) -> bool {
        let Some(message) = self.messages.pop_front() else {
            return false;
        };
        let outcome = self.app.drive_frame(
            message,
            &self.ui_tx,
            &SpawnContextRefs { agent_client: None },
        );
        self.messages.extend(self.effects.record(outcome));
        true
    }

    pub fn drain(&mut self, max_steps: usize) {
        let mut steps = 0;
        while self.step() {
            assert!(
                steps < max_steps,
                "scenario exceeded {max_steps} steps\n{}",
                self.diagnostics()
            );
            steps += 1;
        }
    }

    pub fn run_until(&mut self, max_steps: usize, predicate: impl Fn(&Self) -> bool) {
        for _ in 0..max_steps {
            if predicate(self) {
                return;
            }
            if !self.step() {
                break;
            }
        }
        assert!(
            predicate(self),
            "scenario predicate not reached\n{}",
            self.diagnostics()
        );
    }

    pub fn render(&mut self) {
        self.app.view_state.spinner.verb = "Brewing".to_owned();
        self.app.prepare_frame();
        self.app.draw(&mut self.terminal).expect("TestBackend draw");
    }
    pub fn screen(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        let area = buffer.area;
        super::screen::normalize_screen(
            &(0..area.height)
                .map(|y| {
                    (0..area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
    pub fn input_text(&self) -> String {
        self.app.model.input.document.buffer.clone()
    }
    pub fn messages_empty(&self) -> bool {
        self.messages.is_empty()
    }
    pub fn ticks(&self) -> u64 {
        self.ticks
    }
    pub fn effects(&self) -> &[crate::tui::effect::effect::Effect] {
        &self.effects.effects
    }
    pub fn assert_idle(&self) {
        assert!(self.messages.is_empty(), "pending messages remain");
        assert!(
            self.effects.is_idle(),
            "pending scripted/spawn/slash effects remain"
        );
    }
    fn diagnostics(&self) -> String {
        format!(
            "pending_messages={} effects={} ticks={}\nscreen:\n{}",
            self.messages.len(),
            self.effects.effects.len(),
            self.ticks,
            self.screen()
        )
    }
}
