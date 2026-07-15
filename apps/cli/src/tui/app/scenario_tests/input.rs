use crossterm::event::{KeyCode, KeyModifiers};

use super::super::testing::{input, TuiScenarioHarness};

#[test]
fn character_key_updates_model_and_framebuffer() {
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.key(input::press(KeyCode::Char('x'), KeyModifiers::NONE));
    harness.render();

    assert_eq!(harness.input_text(), "x");
    assert!(harness.screen().contains('x'));
    harness.assert_idle();
}
