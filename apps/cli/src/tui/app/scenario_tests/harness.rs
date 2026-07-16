use crate::tui::app::event::UiEvent;
use crate::tui::update::msg::TuiMsg;

use super::super::testing::input;
use super::super::testing::{ExpectedEffect, TuiScenarioHarness};
use crossterm::event::{KeyCode, KeyModifiers};

#[test]
fn scripted_user_message_replies_are_drained() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.expect_effect(ExpectedEffect::SendUserMessage {
        text: "hello".into(),
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("accepted".into()))],
    });

    for ch in "hello".chars() {
        harness.key(input::press(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    harness.render();
    assert!(harness.effects().iter().any(|effect| {
        matches!(effect, crate::tui::effect::effect::Effect::RunHook { name, message }
            if name == "system_message" && message == "accepted")
    }));
    harness.assert_idle();
}
