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
            question_seq: 0,
            question: "Pick A or B".into(),
            options: vec![
                sdk::OptionItem::title_only("A"),
                sdk::OptionItem::title_only("B"),
            ],
            multi_select: false,
            allow_free_input: true,
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
    assert_eq!(
        reply_rx.try_recv().expect("ask reply"),
        sdk::AskUserReply::Answers(vec![sdk::AskUserAnswer {
            tool_call_id: "ask-1".to_string(),
            question_seq: 0,
            answer: "B".to_string(),
        }])
    );
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

#[test]
fn ask_user_free_text_stays_in_ask_block() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel();
    harness.ui(UiEvent::AskUserBatch {
        items: vec![sdk::AskUserQuestionItem {
            id: "ask-free-text".into(),
            question_seq: 0,
            question: "Tell me more".into(),
            options: Vec::new(),
            multi_select: false,
            allow_free_input: true,
            default: None,
        }],
        reply_tx,
    });

    for ch in "222".chars() {
        harness.key(input::press(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert_eq!(
        harness
            .app
            .model
            .conversation
            .ask_user_chat_text()
            .as_deref(),
        Some("222")
    );
    assert!(harness.app.model.input.document.buffer.is_empty());

    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        reply_rx.try_recv().expect("ask reply"),
        sdk::AskUserReply::Answers(vec![sdk::AskUserAnswer {
            tool_call_id: "ask-free-text".to_string(),
            question_seq: 0,
            answer: "222".to_string(),
        }])
    );
    harness.assert_idle();
}

#[test]
fn resume_restores_all_answered_ask_batches() {
    fn ask_tool_use(id: &str, question: &str) -> sdk::ContentBlock {
        sdk::ContentBlock::ToolUse {
            id: id.to_string(),
            name: "AskUserQuestion".to_string(),
            input: serde_json::json!({ "question": question }),
        }
    }
    fn ask_result(id: &str, answer: &str) -> sdk::ChatMessage {
        sdk::ChatMessage {
            role: "user".to_string(),
            content: vec![sdk::ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: serde_json::json!({ "answer": answer }),
                is_error: false,
                text: None,
            }],
            metadata: None,
            input_id: None,
        }
    }

    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.model.conversation.apply(
        crate::tui::model::conversation::intent::ResumeConversation {
            messages: vec![
                sdk::ChatMessage {
                    role: "assistant".to_string(),
                    content: vec![ask_tool_use("resume-ask-1", "恢复问题一")],
                    metadata: None,
                    input_id: None,
                },
                ask_result("resume-ask-1", "恢复答案一"),
                sdk::ChatMessage {
                    role: "assistant".to_string(),
                    content: vec![ask_tool_use("resume-ask-2", "恢复问题二")],
                    metadata: None,
                    input_id: None,
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
