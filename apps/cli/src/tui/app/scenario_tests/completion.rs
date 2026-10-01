use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::model::input::completion_item::CompletionItem;
use crate::tui::model::input::intent::InputIntent;

use super::super::testing::{input, ExpectedEffect, TuiScenarioHarness};

fn open_help_completion(harness: &mut TuiScenarioHarness) {
    harness
        .app
        .model
        .input
        .apply(InputIntent::ReplaceText("/he".into()));
    harness
        .app
        .handle_input_intent(InputIntent::SetCompletions {
            query: "/he".into(),
            items: vec![
                CompletionItem::new("/help", "/help"),
                CompletionItem::new("/hooks", "/hooks"),
            ],
        });
}

#[test]
fn idle_and_busy_escape_close_completion_before_other_actions() {
    for busy in [false, true] {
        let mut harness = TuiScenarioHarness::new(100, 30);
        if busy {
            harness.app.chat.start_processing();
        }
        open_help_completion(&mut harness);
        harness.render();
        assert!(harness.screen().contains("/help"));
        harness.key(input::press(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!harness.app.model.input.completion.visible);
        assert!(!harness
            .effects()
            .iter()
            .any(|effect| matches!(effect, crate::tui::effect::effect::Effect::CancelCurrentRun)));
        harness.assert_idle();
    }
}

#[test]
fn busy_tab_moves_between_completion_candidates() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    open_help_completion(&mut harness);
    harness.key(input::press(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(harness.app.model.input.document.buffer, "/help");
    assert!(!harness.app.model.input.completion.visible);
}

#[test]
fn busy_enter_accepts_visible_completion_instead_of_submitting_partial_command() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    open_help_completion(&mut harness);

    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(harness.app.model.input.document.buffer, "/help");
    assert!(!harness.app.model.input.completion.visible);
    assert!(!harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::SendChatInputEvent { .. }
    )));
}

#[test]
fn busy_escape_without_completion_cancels_once() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    harness.expect_effect(ExpectedEffect::CancelCurrentRun { replies: vec![] });
    harness.key(input::press(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        harness
            .effects()
            .iter()
            .filter(|effect| matches!(effect, crate::tui::effect::effect::Effect::CancelCurrentRun))
            .count(),
        1
    );
    harness.assert_idle();
}
