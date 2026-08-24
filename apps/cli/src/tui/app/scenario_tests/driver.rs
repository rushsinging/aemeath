use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;

use super::super::testing::TuiScenarioHarness;

#[test]
fn runtime_event_run_until_and_tick_are_deterministic() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::SystemMessage("runtime-event".into()));
    harness.run_until(4, |harness| harness.messages_empty());
    harness.tick();

    assert_eq!(harness.ticks(), 1);
    assert_eq!(
        harness
            .app
            .model
            .conversation
            .timeline
            .items()
            .iter()
            .filter(|item| matches!(
                item,
                crate::tui::model::output_timeline::OutputTimelineItem::System {
                    text,
                    ..
                } if text == "runtime-event"
            ))
            .count(),
        1
    );
    harness.assert_idle();
}
