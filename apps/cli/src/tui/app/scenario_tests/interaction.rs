use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::app::event::UiEvent;

use super::super::testing::{input, ExpectedEffect, TuiScenarioHarness};

#[test]
fn ask_user_selects_option_and_submits_reply() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel();
    harness.ui(UiEvent::AskUserBatch {
        items: vec![sdk::AskUserQuestionItem {
            id: "ask-1".into(),
            question: "Pick A or B".into(),
            options: vec![
                sdk::OptionItem::title_only("A"),
                sdk::OptionItem::title_only("B"),
            ],
            multi_select: false,
            default: None,
        }],
        reply_tx,
    });
    harness.render();
    assert!(harness.screen().contains("Pick A or B"));
    insta::assert_snapshot!("ask_user__shown__100x30", harness.screen());

    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(reply_rx.try_recv().expect("ask reply"), vec!["B"]);
    harness.render();
    insta::assert_snapshot!("ask_user__confirmed__100x30", harness.screen());
    harness.assert_idle();
}

#[test]
fn cancel_and_quit_effects_are_explicit() {
    let mut busy = TuiScenarioHarness::new(100, 30);
    busy.app.chat.start_processing();
    busy.expect_effect(ExpectedEffect::CancelCurrentRun {
        replies: vec![TuiMsg::Ui(UiEvent::RunCancelled)],
    });
    busy.key(input::press(KeyCode::Esc, KeyModifiers::NONE));
    assert!(busy
        .effects()
        .iter()
        .any(|effect| matches!(effect, crate::tui::effect::effect::Effect::CancelCurrentRun)));
    busy.assert_idle();

    let mut idle = TuiScenarioHarness::new(100, 30);
    idle.expect_effect(ExpectedEffect::QuitApplication);
    idle.key(input::press(KeyCode::Char('c'), KeyModifiers::CONTROL));
    idle.key(input::press(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(idle
        .effects()
        .iter()
        .any(|effect| matches!(effect, crate::tui::effect::effect::Effect::QuitApplication)));
    idle.assert_idle();
}

use crate::tui::update::msg::TuiMsg;
