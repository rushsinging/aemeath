use crate::tui::adapter::tui_runtime_event::{TuiRunContext, TuiRuntimeEvent};
use crate::tui::effect::effect::Effect;

use super::super::testing::TuiScenarioHarness;

fn ctx() -> TuiRunContext {
    TuiRunContext {
        chat_id: "chat-osc".to_string(),
        run_id: "turn-osc".to_string(),
    }
}

fn notification_effects(harness: &TuiScenarioHarness) -> Vec<(String, String)> {
    harness
        .effects()
        .iter()
        .filter_map(|effect| match effect {
            Effect::SendTerminalNotification { title, body } => Some((title.clone(), body.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn done_event_emits_terminal_notification_with_duration() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Done {
        context: ctx(),
        duration_ms: Some(125_000),
    });
    harness.render();

    let notifications = notification_effects(&harness);
    assert_eq!(
        notifications,
        vec![("aemeath".to_string(), "Turn complete in 2m 5s".to_string())],
        "Done 必须产出且仅产出一条 OSC 通知"
    );
    harness.assert_idle();
}

#[test]
fn done_event_without_duration_emits_terminal_notification() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Done {
        context: ctx(),
        duration_ms: None,
    });
    harness.render();

    let notifications = notification_effects(&harness);
    assert_eq!(
        notifications,
        vec![("aemeath".to_string(), "Turn complete".to_string())],
        "无耗时时 Done 仍须产出 OSC 通知"
    );
    harness.assert_idle();
}

#[test]
fn cancelled_event_does_not_emit_terminal_notification() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Cancelled {
        context: ctx(),
        duration_ms: 125_000,
    });
    harness.render();

    assert!(
        notification_effects(&harness).is_empty(),
        "用户主动取消不是回合完成，NEVER 发送通知"
    );
    harness.assert_idle();
}
