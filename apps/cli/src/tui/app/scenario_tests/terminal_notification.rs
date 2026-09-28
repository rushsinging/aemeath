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

/// 完整 session 上下文：项目名进 title，prompt + 分支进 body。
#[test]
fn done_notification_carries_session_context() {
    let mut harness = TuiScenarioHarness::new(100, 30);

    // 项目名：WorkspaceSnapshot → ApplySnapshot。
    harness.runtime_event(TuiRuntimeEvent::WorkspaceSnapshot(
        crate::tui::adapter::tui_runtime_event::TuiWorkspaceSnapshot {
            path_base: "~/repo".to_string(),
            workspace_root: "/repo".to_string(),
            context_stack: vec![],
        },
    ));
    // 分支：metadata 解析回灌（ApplyMetadata 根因匹配 root + revision=1）。
    harness.ui(crate::tui::app::event::UiEvent::WorkspaceMetadataResolved(
        crate::tui::app::event::WorkspaceMetadataResolved {
            root: "/repo".to_string(),
            revision: 1,
            branch: Some("main".to_string()),
            kind: crate::tui::model::conversation::workspace::WorktreeKind::MainCheckout,
        },
    ));
    // 当前 prompt：用户消息 adopted 进 timeline。
    harness.runtime_event(TuiRuntimeEvent::UserMessagesAdopted {
        items: vec![
            crate::tui::adapter::runtime_view::TuiChatMessage::user_text(
                "重构通知逻辑\n第二行不该出现",
            ),
        ],
        queued: vec![],
    });

    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Done {
        context: ctx(),
        duration_ms: Some(125_000),
    });
    harness.render();

    assert_eq!(
        notification_effects(&harness),
        vec![(
            "aemeath · ~/repo".to_string(),
            "Turn complete in 2m 5s · main · 重构通知逻辑".to_string()
        )],
        "通知必须携带项目名、prompt 首行与分支，完成状态在正文最前"
    );
    harness.assert_idle();
}
