use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock, TuiMessageSource};
use crate::tui::app::event::UiEvent;
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
