use crate::tui::app::event::UiEvent;

use super::super::testing::TuiScenarioHarness;

#[test]
fn runtime_event_run_until_and_tick_are_deterministic() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime(UiEvent::SystemMessage("runtime-event".into()));
    harness.run_until(4, |harness| harness.messages_empty());
    harness.tick();

    assert_eq!(harness.ticks(), 1);
    assert!(harness.effects().iter().any(|effect| {
        matches!(effect, crate::tui::effect::effect::Effect::RunHook { message, .. }
            if message == "runtime-event")
    }));
    harness.assert_idle();
}
