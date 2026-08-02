use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use crossterm::event::{KeyCode, KeyModifiers};

use super::super::testing::{input, TuiScenarioHarness};

fn sdk_context() -> sdk::ChatEventContext {
    sdk::ChatEventContext::new(
        sdk::ids::ChatId::new("resume-sdk-chat"),
        sdk::ids::ChatTurnId::new("resume-sdk-turn"),
    )
}

#[test]
fn sdk_events_after_resumed_history_are_batched_reduced_and_rendered_at_tail() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..2_000)
        .map(
            |index| crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "resume-sdk-run".into(),
                step_id: format!("resume-sdk-step-{index}"),
                messages: vec![
                    crate::tui::adapter::runtime_view::TuiChatMessage::assistant_text(format!(
                        "RESUME-SDK-HISTORY-{index:04}"
                    )),
                ],
                finalize_cause: None,
                duration_ms: None,
            },
        )
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps,
        session_id: "resume-sdk-session".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();
    while harness.app.view_state.output.render_line_limit() < 3_000 {
        harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
        harness.render();
    }
    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();
    assert!(harness.app.view_state.output.history_window_tail_offset > 0);
    assert!(!harness.app.view_state.output.auto_scroll);

    let context = sdk_context();
    harness.sdk_runtime_batch([
        sdk::ChatEvent::UserMessagesAdopted {
            items: vec![sdk::ChatMessage::user_text("RESUME-SDK-NEW-USER")],
            queued: vec![],
        },
        sdk::ChatEvent::Token {
            context: context.clone(),
            text: "RESUME-SDK-NEXT-".into(),
        },
        sdk::ChatEvent::Token {
            context: context.clone(),
            text: "ASSISTANT".into(),
        },
        sdk::ChatEvent::BlockComplete {
            context,
            text: "RESUME-SDK-NEXT-ASSISTANT".into(),
        },
    ]);
    harness.render();

    assert_eq!(harness.app.view_state.output.history_window_tail_offset, 0);
    assert!(harness.app.view_state.output.auto_scroll);
    let screen = harness.screen();
    assert!(screen.contains("RESUME-SDK-NEW-USER"));
    assert!(screen.contains("RESUME-SDK-NEXT-ASSISTANT"));
    assert!(harness
        .app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .any(|item| matches!(
            item,
            crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. }
                if text == "RESUME-SDK-NEW-USER"
        )));
    assert!(harness
        .app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .any(|item| matches!(
            item,
            crate::tui::model::output_timeline::OutputTimelineItem::AssistantText { text, .. }
                if text == "RESUME-SDK-NEXT-ASSISTANT"
        )));
}
