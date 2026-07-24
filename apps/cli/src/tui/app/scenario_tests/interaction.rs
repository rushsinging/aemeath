use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiMessageSource, TuiResumedSessionStep,
};
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
fn resume_renders_context_run_steps_without_inventing_chats_from_user_messages() {
    let mut harness = TuiScenarioHarness::new(100, 30);

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps: vec![
            TuiResumedSessionStep {
                run_id: "run-1".into(),
                step_id: "step-1".into(),
                messages: vec![
                    TuiChatMessage::user_text("QUESTION_ONE"),
                    TuiChatMessage::assistant_text("ANSWER_ONE"),
                ],
            },
            TuiResumedSessionStep {
                run_id: "run-1".into(),
                step_id: "step-2".into(),
                messages: vec![
                    TuiChatMessage::user_text("QUESTION_TWO"),
                    TuiChatMessage::assistant_text("ANSWER_TWO"),
                ],
            },
        ],
        session_id: "session-resumed".into(),
        created_at: 0,
    });
    harness.render();

    assert_eq!(harness.app.model.conversation.chats.len(), 1);
    let chat = &harness.app.model.conversation.chats[0];
    assert_eq!(chat.id.as_str(), "run-1");
    assert_eq!(chat.turns.len(), 2);
    assert_eq!(chat.turns[0].id.as_str(), "step-1");
    assert_eq!(chat.turns[1].id.as_str(), "step-2");
    let screen = harness.screen();
    for expected in ["QUESTION_ONE", "ANSWER_ONE", "QUESTION_TWO", "ANSWER_TWO"] {
        assert!(
            screen.contains(expected),
            "resume framebuffer 缺少 {expected}\n{screen}"
        );
    }
    assert!(!harness.app.model.conversation.runtime.spinner.chat_active);

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps: vec![TuiResumedSessionStep {
            run_id: "run-2".into(),
            step_id: "step-1".into(),
            messages: vec![TuiChatMessage::user_text("ANOTHER_RUN")],
        }],
        session_id: "session-resumed".into(),
        created_at: 0,
    });
    assert_eq!(harness.app.model.conversation.chats.len(), 1);
    assert_eq!(harness.app.model.conversation.chats[0].id.as_str(), "run-2");
    assert!(harness
        .app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .all(|item| !matches!(
            item,
            crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. }
                if text == "QUESTION_ONE" || text == "QUESTION_TWO"
        )));
}

#[test]
fn resume_renders_bash_tool_with_typed_header_and_output() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps: vec![TuiResumedSessionStep {
            run_id: "run-bash".into(),
            step_id: "step-bash".into(),
            messages: vec![
                TuiChatMessage {
                    role: "assistant".into(),
                    content: vec![TuiContentBlock::ToolUse {
                        id: "bash-1".into(),
                        name: "Bash".into(),
                        input: serde_json::json!({ "command": "git status --short --branch" }),
                    }],
                    input_id: None,
                    source: TuiMessageSource::User,
                    stop_hook: None,
                },
                TuiChatMessage {
                    role: "user".into(),
                    content: vec![TuiContentBlock::ToolResult {
                        tool_use_id: "bash-1".into(),
                        content: serde_json::json!({
                            "stdout": "## feature/resume...origin/main",
                            "stderr": "",
                            "exit_code": 0,
                            "signal": null,
                            "path_base": "/repo"
                        }),
                        is_error: false,
                        text: Some("## feature/resume...origin/main\n[cwd: /repo]".into()),
                    }],
                    input_id: None,
                    source: TuiMessageSource::User,
                    stop_hook: None,
                },
            ],
        }],
        session_id: "session-bash".into(),
        created_at: 0,
    });
    harness.render();

    let screen = harness.screen();
    assert!(
        screen.contains("Run git status --short --branch"),
        "resume 后 Bash 应走 typed ToolDisplay header\n{screen}"
    );
    assert_eq!(
        screen.matches("git status --short --branch").count(),
        1,
        "Bash 命令在 header 已显示，不能再重复为 details\n{screen}"
    );
    assert!(
        screen.contains("## feature/resume...origin/main"),
        "resume 后 Bash 应显示原始 stdout，而不是结构化 JSON\n{screen}"
    );
    assert!(
        !screen.contains("{\"exit_code\""),
        "resume 后不得把 BashResult JSON 直接刷到 TUI\n{screen}"
    );
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
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "history-run".into(),
                step_id: "history-step".into(),
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
            }],
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
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: request_id.clone(),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "中午吃什么?".to_string(),
                options: vec!["饺子".to_string(), "拉面".to_string(), "盖浇饭".to_string()],
                allow_multi: false,
            }]),
        },
    ));
    harness.render();

    // AskUserBatch block should be in the timeline
    assert!(harness.app.model.conversation.ask_user_snapshot().is_some());

    // Script the expected ReplyInteraction effect
    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("answered".into()))],
    });

    // Navigate and confirm: Enter on the first option
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    // Verify the reply effect was emitted
    assert!(harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::ReplyInteraction { .. }
    )));
    harness.assert_idle();
}

#[test]
fn ask_user_cancel_emits_cancel_interaction_effect() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    let request_id = UiInteractionRequestId::from("test-ask-cancel");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: request_id.clone(),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "确认删除?".to_string(),
                options: vec!["是".to_string(), "否".to_string()],
                allow_multi: false,
            }]),
        },
    ));
    harness.render();

    assert!(harness.app.model.conversation.ask_user_snapshot().is_some());

    harness.expect_effect(ExpectedEffect::CancelInteraction {
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("cancelled".into()))],
    });

    // Ctrl+C cancels the interaction
    harness.key(input::press(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert!(harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::CancelInteraction { .. }
    )));
    harness.assert_idle();
}

#[test]
fn ask_user_esc_during_chat_input_exits_chat_mode_not_cancel() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    let request_id = UiInteractionRequestId::from("test-esc-chat");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id,
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "自由输入".to_string(),
                options: vec!["选项A".to_string(), "选项B".to_string()],
                allow_multi: false,
            }]),
        },
    ));
    harness.render();

    // Navigate to "Type something..." (last option, index = llm_option_count = 2)
    // Up to last item then Enter to activate chat-input mode
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    // Now in chat-input mode: Esc exits chat-input (not cancel)
    harness.key(input::press(KeyCode::Esc, KeyModifiers::NONE));

    // Should still have ask_user batch (not cancelled)
    assert!(harness.app.model.conversation.ask_user_snapshot().is_some());
    harness.assert_idle();
}
