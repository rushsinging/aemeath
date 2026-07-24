use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock, TuiMessageSource};
use crate::tui::adapter::tui_runtime_event::{
    TuiInteractionBody, TuiInteractionRequest, TuiRuntimeEvent, TuiUserQuestion,
};
use crate::tui::app::event::UiEvent;
use crate::tui::model::conversation::interaction::UiInteractionRequestId;
use crate::tui::update::msg::TuiMsg;

use super::super::testing::{input, ExpectedEffect, TuiScenarioHarness};

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

#[test]
fn resume_restores_all_answered_ask_batches() {
    fn ask_tool_use(id: &str, question: &str) -> TuiContentBlock {
        TuiContentBlock::ToolUse {
            id: id.to_string(),
            name: "AskUserQuestion".to_string(),
            input: serde_json::json!({ "question": question }),
        }
    }
    fn ask_result(id: &str, answer: &str) -> TuiChatMessage {
        TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: serde_json::json!({ "answer": answer }),
                is_error: false,
                text: None,
            }],
            input_id: None,
            source: TuiMessageSource::User,
            stop_hook: None,
        }
    }

    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.model.conversation.apply(
        crate::tui::model::conversation::intent::ResumeConversation {
            messages: vec![
                TuiChatMessage {
                    role: "assistant".to_string(),
                    content: vec![ask_tool_use("resume-ask-1", "恢复问题一")],
                    input_id: None,
                    source: TuiMessageSource::User,
                    stop_hook: None,
                },
                ask_result("resume-ask-1", "恢复答案一"),
                TuiChatMessage {
                    role: "assistant".to_string(),
                    content: vec![ask_tool_use("resume-ask-2", "恢复问题二")],
                    input_id: None,
                    source: TuiMessageSource::User,
                    stop_hook: None,
                },
                ask_result("resume-ask-2", "恢复答案二"),
            ],
        },
    );
    let restored_count = harness
        .app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter(|item| {
            matches!(
                item,
                crate::tui::model::output_timeline::OutputTimelineItem::AskUserBatch {
                    confirmed: true,
                    ..
                }
            )
        })
        .count();
    assert_eq!(restored_count, 2);
    harness.app.view_state.output.auto_scroll = false;
    harness.app.mark_output_dirty();
    harness.render();
    let screen = harness.screen();
    assert!(!screen.is_empty());
    assert_eq!(restored_count, 2);
}

#[test]
fn ask_user_confirm_emits_reply_interaction_effect() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    // Simulate InteractionRequested runtime event
    let request_id = UiInteractionRequestId::from("test-ask-1");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(TuiInteractionRequest {
        request_id: request_id.clone(),
        run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
        body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
            prompt: "中午吃什么?".to_string(),
            options: vec!["饺子".to_string(), "拉面".to_string(), "盖浇饭".to_string()],
            allow_multi: false,
        }]),
    }));
    harness.render();

    // AskUserBatch block should be in the timeline
    assert!(harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .is_some());

    // Script the expected ReplyInteraction effect
    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("answered".into()))],
    });

    // Navigate and confirm: Enter on the first option
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    // Verify the reply effect was emitted
    assert!(harness
        .effects()
        .iter()
        .any(|effect| matches!(effect, crate::tui::effect::effect::Effect::ReplyInteraction { .. })));
    harness.assert_idle();
}

#[test]
fn ask_user_cancel_emits_cancel_interaction_effect() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    let request_id = UiInteractionRequestId::from("test-ask-cancel");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(TuiInteractionRequest {
        request_id: request_id.clone(),
        run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
        body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
            prompt: "确认删除?".to_string(),
            options: vec!["是".to_string(), "否".to_string()],
            allow_multi: false,
        }]),
    }));
    harness.render();

    assert!(harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .is_some());

    harness.expect_effect(ExpectedEffect::CancelInteraction {
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("cancelled".into()))],
    });

    // Ctrl+C cancels the interaction
    harness.key(input::press(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert!(harness
        .effects()
        .iter()
        .any(|effect| matches!(effect, crate::tui::effect::effect::Effect::CancelInteraction { .. })));
    harness.assert_idle();
}

#[test]
fn ask_user_esc_during_chat_input_exits_chat_mode_not_cancel() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    let request_id = UiInteractionRequestId::from("test-esc-chat");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(TuiInteractionRequest {
        request_id,
        run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
        body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
            prompt: "自由输入".to_string(),
            options: vec!["选项A".to_string(), "选项B".to_string()],
            allow_multi: false,
        }]),
    }));
    harness.render();

    // Navigate to "Type something..." (last option, index = llm_option_count = 2)
    // Up to last item then Enter to activate chat-input mode
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    // Now in chat-input mode: Esc exits chat-input (not cancel)
    harness.key(input::press(KeyCode::Esc, KeyModifiers::NONE));

    // Should still have ask_user batch (not cancelled)
    assert!(harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .is_some());
    harness.assert_idle();
}
