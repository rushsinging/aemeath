use std::sync::Arc;

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiMessageSource, TuiResumedSessionStep,
};
use crate::tui::adapter::tui_runtime_event::{
    TuiInteractionBody, TuiInteractionRequest, TuiOptionItem, TuiRunContext, TuiRuntimeEvent,
    TuiToolCallStatus, TuiUserQuestion,
};
use crate::tui::app::event::UiEvent;
use crate::tui::model::conversation::interaction::UiInteractionRequestId;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::update::msg::TuiMsg;

use super::super::testing::{input, ExpectedEffect, TuiScenarioHarness};

#[test]
fn cancel_and_quit_effects_are_explicit() {
    let mut busy = TuiScenarioHarness::new(100, 30);
    busy.app.chat.start_processing();
    let run_id = sdk::RunId::from_legacy_or_new("run-cancel");
    let step_id = sdk::RunStepId::from_legacy_or_new("step-cancel");
    busy.app.chat.active_run_step = Some((run_id.clone(), step_id.clone()));
    busy.expect_effect(ExpectedEffect::CancelRunStep {
        run_id,
        step_id,
        replies: vec![],
    });
    busy.key(input::press(KeyCode::Esc, KeyModifiers::NONE));
    assert!(busy.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::CancelRunStep { .. }
    )));
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
        display_history: None,
        steps: vec![
            TuiResumedSessionStep {
                run_id: "run-1".into(),
                step_id: "step-1".into(),
                messages: vec![
                    TuiChatMessage::user_text("QUESTION_ONE"),
                    TuiChatMessage::assistant_text("ANSWER_ONE"),
                ],
                finalize_cause: None,
                duration_ms: None,
            },
            TuiResumedSessionStep {
                run_id: "run-1".into(),
                step_id: "step-2".into(),
                messages: vec![
                    TuiChatMessage::user_text("QUESTION_TWO"),
                    TuiChatMessage::assistant_text("ANSWER_TWO"),
                ],
                finalize_cause: None,
                duration_ms: None,
            },
        ],
        session_id: "session-resumed".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();

    assert_eq!(harness.app.model.conversation.chats.len(), 1);
    let chat = &harness.app.model.conversation.chats[0];
    assert_eq!(chat.id.as_str(), "run-1");
    assert_eq!(chat.runs.len(), 2);
    assert_eq!(chat.runs[0].id.as_str(), "step-1");
    assert_eq!(chat.runs[1].id.as_str(), "step-2");
    let screen = harness.screen();
    for expected in ["QUESTION_ONE", "ANSWER_ONE", "QUESTION_TWO", "ANSWER_TWO"] {
        assert!(
            screen.contains(expected),
            "resume framebuffer 缺少 {expected}\n{screen}"
        );
    }
    assert!(harness
        .app
        .model
        .conversation
        .activity_observations()
        .activities()
        .is_empty());

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps: vec![TuiResumedSessionStep {
            run_id: "run-2".into(),
            step_id: "step-1".into(),
            messages: vec![TuiChatMessage::user_text("ANOTHER_RUN")],
            finalize_cause: None,
            duration_ms: None,
        }],
        session_id: "session-resumed".into(),
        created_at: 0,
        compacted: false,
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
        display_history: None,
        steps: vec![TuiResumedSessionStep {
            run_id: "run-bash".into(),
            step_id: "step-bash".into(),
            messages: vec![
                TuiChatMessage {
                    role: "assistant".into(),
                    content: vec![TuiContentBlock::ToolUse {
                        id: "bash-1".into(),
                        name: "Bash".into(),
                        input: serde_json::json!({
                            "goal": "查看分支状态",
                            "command": "git status --short --branch"
                        }),
                    }],
                    input_id: None,
                    source: TuiMessageSource::User,
                    hook_notice: None,
                    skill_request: None,
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
                    hook_notice: None,
                    skill_request: None,
                },
            ],
            finalize_cause: None,
            duration_ms: None,
        }],
        session_id: "session-bash".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();

    let screen = harness.screen();
    assert!(
        screen.contains("查 看 分 支 状 态"),
        "resume 后 Bash header 应显示 goal\n{screen}"
    );
    assert!(
        screen.contains("git status --short --branch"),
        "Bash 命令全文应在 details 中显示\n{screen}"
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
            hook_notice: None,
            skill_request: None,
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
                        hook_notice: None,
                        skill_request: None,
                    },
                    ask_result("resume-ask-1", "恢复答案一"),
                    TuiChatMessage {
                        role: "assistant".to_string(),
                        content: vec![ask_tool_use("resume-ask-2", "恢复问题二")],
                        input_id: None,
                        source: TuiMessageSource::User,
                        hook_notice: None,
                        skill_request: None,
                    },
                    ask_result("resume-ask-2", "恢复答案二"),
                ],
                finalize_cause: None,
                duration_ms: None,
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
                    completion: crate::tui::model::conversation::block::AskUserCompletion::Answered,
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

#[tokio::test]
async fn ask_user_accepted_reply_marks_the_ask_tool_gutter_completed_end_to_end() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    let request_id = UiInteractionRequestId::from("018f0000-0000-7000-8000-000000000012");
    harness.app.agent_client = Some(Arc::new(AcceptingInteractionClient));
    let context = TuiRunContext {
        chat_id: "ask-chat".to_string(),
        run_id: "ask-turn".to_string(),
    };
    let tool_call_id = crate::tui::model::conversation::ids::ToolCallId::new("ask-tool-call");
    harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
        context: context.clone(),
        id: tool_call_id.as_str().to_string(),
        provider_id: Some(tool_call_id.as_str().to_string()),
        name: "AskUserQuestion".to_string(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallStateChanged {
        context,
        id: tool_call_id.as_str().to_string(),
        provider_id: Some(tool_call_id.as_str().to_string()),
        name: "AskUserQuestion".to_string(),
        index: 0,
        arguments: Some(serde_json::json!({ "question": "明天想吃什么？", "options": ["日料"] })),
        status: TuiToolCallStatus::Running,
    });
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: request_id.clone(),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            tool_call_id: Some(tool_call_id.as_str().to_string()),
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "明天想吃什么？".to_string(),
                options: vec![TuiOptionItem {
                    title: "日料".to_string(),
                    description: Some("日料的描述".to_string()),
                }],
                allow_multi: false,
            }]),
        },
    ));
    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        request_id: Some(request_id.as_str().to_string()),
        reply: Some(
            crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(vec![
                "日料".to_string(),
            ]),
        ),
        replies: Vec::new(),
    });

    // 选项 description 必须从 TUI 事件一路渲染到屏幕（全链路不丢失）。
    // 屏幕文本对 CJK 字符间插入全角间距，断言前先去除空格。
    harness.render();
    let screen_with_options = harness.screen();
    assert!(
        screen_with_options
            .lines()
            .any(|line| line.replace(' ', "").contains("日料的描述")),
        "选项 description 应渲染在屏幕上\n{screen_with_options}"
    );

    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    harness.execute_last_effect().await; // allow tea_side_effect: scenario drives the production effect executor
    harness.render();
    assert!(harness
        .app
        .model
        .conversation
        .active_interaction()
        .is_none());
    let ask_tool = harness
        .app
        .model
        .conversation
        .chats
        .iter()
        .flat_map(|chat| &chat.runs)
        .flat_map(|turn| &turn.tool_calls)
        .find(|call| call.id.as_ref() == Some(&tool_call_id))
        .expect("AskUserQuestion tool call should exist");
    assert_eq!(ask_tool.status, ToolCallStatus::Success);
    let result = ask_tool
        .result
        .as_ref()
        .expect("accepted AskUserQuestion should have a result payload");
    assert_eq!(result.output, "Q1: 日料");
    assert_eq!(
        result.content,
        serde_json::json!({"status": "ok", "answers": ["日料"]})
    );
    assert!(!result.is_error);
    let screen = harness.screen();
    assert!(
        screen
            .lines()
            .any(|line| line.contains('✓') && line.contains("Ask")),
        "Accepted 后成功 gutter 应位于 Ask 工具调用行\n{screen}"
    );
    assert!(
        screen
            .lines()
            .filter(|line| line.contains("已回答"))
            .all(|line| !line.contains('✓')),
        "已回答摘要前不应显示成功 gutter\n{screen}"
    );
    harness.assert_idle();
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
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "中午吃什么?".to_string(),
                options: vec![
                    TuiOptionItem {
                        title: "饺子".to_string(),
                        description: Some("饺子的描述".to_string()),
                    },
                    TuiOptionItem {
                        title: "拉面".to_string(),
                        description: Some("拉面的描述".to_string()),
                    },
                    TuiOptionItem {
                        title: "盖浇饭".to_string(),
                        description: Some("盖浇饭的描述".to_string()),
                    },
                ],
                allow_multi: false,
            }]),
        },
    ));
    harness.render();

    // AskUserBatch block should be in the timeline
    assert!(harness.app.model.conversation.ask_user_snapshot().is_some());

    // Script the expected ReplyInteraction effect
    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        request_id: Some("test-ask-1".to_string()),
        reply: Some(
            crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(vec![
                "饺子".to_string(),
            ]),
        ),
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
fn ask_user_free_text_confirmation_emits_reply_interaction_effect() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    let request_id = UiInteractionRequestId::from("test-ask-free-text");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id,
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "中午吃什么?".to_string(),
                options: vec![
                    TuiOptionItem {
                        title: "饺子".to_string(),
                        description: Some("饺子的描述".to_string()),
                    },
                    TuiOptionItem {
                        title: "拉面".to_string(),
                        description: Some("拉面的描述".to_string()),
                    },
                ],
                allow_multi: false,
            }]),
        },
    ));

    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        request_id: Some("test-ask-free-text".to_string()),
        reply: Some(
            crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(vec![
                "日料".to_string(),
            ]),
        ),
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("answered".into()))],
    });

    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    for ch in "日料".chars() {
        harness.key(input::press(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    assert!(harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::ReplyInteraction {
            reply: crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(answers),
            ..
        } if answers == &vec!["日料".to_string()]
    )));
    harness.assert_idle();
}

#[test]
fn ask_user_current_interaction_does_not_reply_with_resumed_history_answer() {
    fn ask_tool_use(id: &str, question: &str) -> TuiContentBlock {
        TuiContentBlock::ToolUse {
            id: id.to_string(),
            name: "AskUserQuestion".to_string(),
            input: serde_json::json!({ "question": question }),
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
                        content: vec![ask_tool_use("history-ask", "之前想吃什么？")],
                        input_id: None,
                        source: TuiMessageSource::User,
                        hook_notice: None,
                        skill_request: None,
                    },
                    TuiChatMessage {
                        role: "user".to_string(),
                        content: vec![TuiContentBlock::ToolResult {
                            tool_use_id: "history-ask".to_string(),
                            content: serde_json::json!({ "answer": "中餐" }),
                            is_error: false,
                            text: None,
                        }],
                        input_id: None,
                        source: TuiMessageSource::User,
                        hook_notice: None,
                        skill_request: None,
                    },
                ],
                finalize_cause: None,
                duration_ms: None,
            }],
        },
    );
    harness.app.chat.start_processing();
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: UiInteractionRequestId::from("current-ask"),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("current-run"),
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "明天想吃什么？".to_string(),
                options: vec![
                    TuiOptionItem {
                        title: "日料".to_string(),
                        description: Some("日料的描述".to_string()),
                    },
                    TuiOptionItem {
                        title: "西餐".to_string(),
                        description: Some("西餐的描述".to_string()),
                    },
                ],
                allow_multi: false,
            }]),
        },
    ));
    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        request_id: Some("current-ask".to_string()),
        reply: Some(
            crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(vec![
                "日料".to_string(),
            ]),
        ),
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("answered".into()))],
    });

    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    assert!(harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::ReplyInteraction {
            request_id,
            reply: crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(answers),
        } if request_id.as_str() == "current-ask" && answers == &vec!["日料".to_string()]
    )));
    harness.assert_idle();
}

struct AcceptingInteractionClient;

#[async_trait]
impl sdk::AgentClient for AcceptingInteractionClient {
    fn reply_interaction(
        &self,
        _request_id: &sdk::InteractionRequestId,
        _reply: sdk::InteractionReply,
    ) -> sdk::InteractionCommandOutcome {
        sdk::InteractionCommandOutcome::Accepted
    }

    fn cancel_interaction(
        &self,
        _request_id: &sdk::InteractionRequestId,
        _reason: sdk::InteractionCancelReason,
    ) -> sdk::InteractionCommandOutcome {
        sdk::InteractionCommandOutcome::Accepted
    }

    async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
        unreachable!("AskUser scenario does not start chat")
    }
}

#[tokio::test]
async fn ask_user_accepted_cancel_marks_the_ask_tool_gutter_cancelled_end_to_end() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    let request_id = UiInteractionRequestId::from("018f0000-0000-7000-8000-000000000011");
    harness.app.agent_client = Some(Arc::new(AcceptingInteractionClient));
    let context = TuiRunContext {
        chat_id: "ask-chat-cancelled".to_string(),
        run_id: "ask-turn-cancelled".to_string(),
    };
    let tool_call_id = crate::tui::model::conversation::ids::ToolCallId::new("ask-tool-cancelled");
    harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
        context: context.clone(),
        id: tool_call_id.as_str().to_string(),
        provider_id: Some(tool_call_id.as_str().to_string()),
        name: "AskUserQuestion".to_string(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallStateChanged {
        context,
        id: tool_call_id.as_str().to_string(),
        provider_id: Some(tool_call_id.as_str().to_string()),
        name: "AskUserQuestion".to_string(),
        index: 0,
        arguments: Some(serde_json::json!({ "question": "确认取消？", "options": ["继续"] })),
        status: TuiToolCallStatus::Running,
    });
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: request_id.clone(),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            tool_call_id: Some(tool_call_id.as_str().to_string()),
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "确认取消？".to_string(),
                options: vec![TuiOptionItem {
                    title: "继续".to_string(),
                    description: Some("继续的描述".to_string()),
                }],
                allow_multi: false,
            }]),
        },
    ));
    harness.expect_effect(ExpectedEffect::CancelInteraction {
        replies: Vec::new(),
    });
    harness.key(input::press(KeyCode::Char('c'), KeyModifiers::CONTROL));
    harness.execute_last_effect().await; // allow tea_side_effect: scenario drives the production effect executor
    harness.render();

    let ask_tool = harness
        .app
        .model
        .conversation
        .chats
        .iter()
        .flat_map(|chat| &chat.runs)
        .flat_map(|turn| &turn.tool_calls)
        .find(|call| call.id.as_ref() == Some(&tool_call_id))
        .expect("AskUserQuestion tool call should exist");
    assert_eq!(ask_tool.status, ToolCallStatus::Cancelled);
    let screen = harness.screen();
    assert!(
        screen
            .lines()
            .any(|line| line.contains('✗') && line.contains("Ask")),
        "accepted cancel 后应显示 cancelled gutter\n{screen}"
    );
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
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "确认删除?".to_string(),
                options: vec![
                    TuiOptionItem {
                        title: "是".to_string(),
                        description: Some("是的描述".to_string()),
                    },
                    TuiOptionItem {
                        title: "否".to_string(),
                        description: Some("否的描述".to_string()),
                    },
                ],
                allow_multi: false,
            }]),
        },
    ));
    harness.render();

    assert!(harness.app.model.conversation.ask_user_snapshot().is_some());

    harness.expect_effect(ExpectedEffect::CancelInteraction {
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("cancelled".into()))],
    });

    harness.key(input::press(KeyCode::Char('c'), KeyModifiers::CONTROL));

    let cancel_effects = harness
        .effects()
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                crate::tui::effect::effect::Effect::CancelInteraction { .. }
            )
        })
        .count();
    assert_eq!(cancel_effects, 1);
    assert!(!harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::CancelRunStep { .. }
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
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "自由输入".to_string(),
                options: vec![
                    TuiOptionItem {
                        title: "选项A".to_string(),
                        description: Some("选项A的描述".to_string()),
                    },
                    TuiOptionItem {
                        title: "选项B".to_string(),
                        description: Some("选项B的描述".to_string()),
                    },
                ],
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

/// 单 tool call 多题 batch 主流程：逐题作答 → 第二题必须可选择 → 确认页提交。
/// 覆盖 issue #1673「多题时第二个问题无法选择」的 model + key + effect 链路。
#[test]
fn ask_user_multi_question_batch_answers_both_questions_in_order() {
    use crate::tui::model::conversation::block::AskUserPhase;

    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    let request_id = UiInteractionRequestId::from("test-ask-multi");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: request_id.clone(),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![
                TuiUserQuestion {
                    prompt: "第一题".to_string(),
                    options: vec![
                        TuiOptionItem {
                            title: "甲".to_string(),
                            description: Some("甲的描述".to_string()),
                        },
                        TuiOptionItem {
                            title: "乙".to_string(),
                            description: Some("乙的描述".to_string()),
                        },
                    ],
                    allow_multi: false,
                },
                TuiUserQuestion {
                    prompt: "第二题".to_string(),
                    options: vec![
                        TuiOptionItem {
                            title: "X".to_string(),
                            description: Some("X的描述".to_string()),
                        },
                        TuiOptionItem {
                            title: "Y".to_string(),
                            description: Some("Y的描述".to_string()),
                        },
                    ],
                    allow_multi: false,
                },
            ]),
        },
    ));
    harness.render();

    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("multi-question batch must be active");
    assert_eq!(snapshot.active_index, 0);
    assert_eq!(snapshot.phase, AskUserPhase::Answering);

    // 第一题：选择「乙」
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch must stay active after first answer");
    assert_eq!(snapshot.active_index, 1, "回答第一题后必须前进到第二题");
    assert_eq!(
        snapshot.phase,
        AskUserPhase::Answering,
        "第二题尚未作答，不得提前进入确认页"
    );

    // 第二题必须可选择：↑↓ 移动选项光标
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch active while answering second question");
    assert_eq!(snapshot.cursor, 1, "第二个问题的选项光标必须可移动");

    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        request_id: Some(request_id.as_str().to_string()),
        reply: Some(
            crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(vec![
                "乙".to_string(),
                "Y".to_string(),
            ]),
        ),
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("answered".into()))],
    });

    // 选择「Y」→ 进入确认页
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch active on confirm page");
    assert_eq!(
        snapshot.phase,
        AskUserPhase::Confirming,
        "两题答完必须进入确认页"
    );

    // 确认页默认停在「全部确认提交」，Enter 提交两题答案
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    assert!(harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::ReplyInteraction {
            reply: crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(answers),
            ..
        } if answers == &vec!["乙".to_string(), "Y".to_string()]
    )));
    harness.assert_idle();
}

/// 第一题经 Type something 自由输入作答后，第二题自由输入必须可继续输入
/// （chat_input_cursor 不得残留上一题的光标位置）。覆盖 issue #1673。
#[test]
fn ask_user_second_question_free_input_accepts_typing_after_first_free_text_answer() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    let request_id = UiInteractionRequestId::from("test-ask-multi-free");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: request_id.clone(),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![
                TuiUserQuestion {
                    prompt: "第一题".to_string(),
                    options: vec![TuiOptionItem {
                        title: "A".to_string(),
                        description: Some("A的描述".to_string()),
                    }],
                    allow_multi: false,
                },
                TuiUserQuestion {
                    prompt: "第二题".to_string(),
                    options: vec![TuiOptionItem {
                        title: "B".to_string(),
                        description: Some("B的描述".to_string()),
                    }],
                    allow_multi: false,
                },
            ]),
        },
    ));
    harness.render();

    // 第一题：进入 Type something 并自由输入作答
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Char('x'), KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch active after first free-text answer");
    assert_eq!(snapshot.active_index, 1, "必须前进到第二题");
    assert_eq!(
        snapshot.chat_input_cursor, 0,
        "切换题目后自由输入光标必须归零，不得残留上一题位置"
    );

    // 第二题：进入 Type something 并输入
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(
        harness
            .app
            .model
            .conversation
            .ask_user_chat_text()
            .as_deref(),
        Some("y"),
        "第二题自由输入框必须接受字符输入"
    );

    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        request_id: Some(request_id.as_str().to_string()),
        reply: Some(
            crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(vec![
                "x".to_string(),
                "y".to_string(),
            ]),
        ),
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("answered".into()))],
    });
    // 第二题自由输入提交 → 确认页 → Enter 提交
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    assert!(harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::ReplyInteraction {
            reply: crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(answers),
            ..
        } if answers == &vec!["x".to_string(), "y".to_string()]
    )));
    harness.assert_idle();
}

/// 无 LLM 选项的问题直接落在 Type something 子态：↑ 与空文本 Enter
/// 都必须能退出子态回到选项列表，不得把交互卡死。覆盖 issue #1673。
#[test]
fn ask_user_chat_input_substate_exits_on_up_arrow_and_empty_enter() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    let request_id = UiInteractionRequestId::from("test-ask-substate");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id,
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "无选项问题".to_string(),
                options: vec![],
                allow_multi: false,
            }]),
        },
    ));
    harness.render();

    // 无 LLM 选项 → cursor 0 即 Type something，Enter 进入子态
    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch active");
    assert_eq!(snapshot.cursor, 0);
    assert!(!snapshot.chat_input_active);
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch active in substate");
    assert!(
        snapshot.chat_input_active,
        "Enter 应进入 Type something 子态"
    );

    // ↑ 必须无条件退出子态（即使选项 cursor 为 0）
    harness.key(input::press(KeyCode::Up, KeyModifiers::NONE));
    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch active after Up");
    assert!(
        !snapshot.chat_input_active,
        "↑ 必须退出 Type something 子态"
    );

    // 重新进入子态，空文本 Enter 必须退出子态
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch active re-entering substate");
    assert!(snapshot.chat_input_active);
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("batch active after empty Enter");
    assert!(
        !snapshot.chat_input_active,
        "空文本 Enter 必须退出子态，不得无响应卡死"
    );
    harness.assert_idle();
}

/// 第一个 request 被 accepted 后，runtime 队列的第二个 InteractionRequested
/// 到达时必须呈现为可选择的新 batch（并行 tool call 场景）。覆盖 issue #1673。
#[tokio::test]
async fn ask_user_second_interaction_request_after_first_accepted_is_selectable() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    harness.app.agent_client = Some(Arc::new(AcceptingInteractionClient));

    // 第一个 request（单题）
    let first_request_id = UiInteractionRequestId::from("018f0000-0000-7000-8000-000000000021");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: first_request_id.clone(),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "第一问".to_string(),
                options: vec![TuiOptionItem {
                    title: "甲".to_string(),
                    description: Some("甲的描述".to_string()),
                }],
                allow_multi: false,
            }]),
        },
    ));
    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        request_id: Some(first_request_id.as_str().to_string()),
        reply: Some(
            crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(vec![
                "甲".to_string(),
            ]),
        ),
        replies: Vec::new(),
    });
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    harness.execute_last_effect().await; // allow tea_side_effect: scenario drives the production effect executor
    harness.render();
    assert!(harness
        .app
        .model
        .conversation
        .active_interaction()
        .is_none());

    // 第二个 request 到达
    let second_request_id = UiInteractionRequestId::from("018f0000-0000-7000-8000-000000000022");
    harness.runtime_event(TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: second_request_id.clone(),
            run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
            tool_call_id: None,
            body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
                prompt: "第二问".to_string(),
                options: vec![TuiOptionItem {
                    title: "乙".to_string(),
                    description: Some("乙的描述".to_string()),
                }],
                allow_multi: false,
            }]),
        },
    ));
    harness.render();

    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("第二个 request 必须呈现为可交互 batch");
    assert_eq!(
        snapshot.completion,
        crate::tui::model::conversation::block::AskUserCompletion::Active
    );
    assert_eq!(snapshot.active_index, 0);

    // 第二个问题必须可选择并提交
    harness.expect_effect(ExpectedEffect::ReplyInteraction {
        request_id: Some(second_request_id.as_str().to_string()),
        reply: Some(
            crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(vec![
                "乙".to_string(),
            ]),
        ),
        replies: vec![TuiMsg::Ui(UiEvent::SystemMessage("answered".into()))],
    });
    harness.key(input::press(KeyCode::Down, KeyModifiers::NONE));
    let snapshot = harness
        .app
        .model
        .conversation
        .ask_user_snapshot()
        .expect("第二个问题交互中");
    assert_eq!(snapshot.cursor, 1, "第二个问题选项光标必须可移动");
    harness.key(input::press(KeyCode::Up, KeyModifiers::NONE));
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    assert!(harness.effects().iter().any(|effect| matches!(
        effect,
        crate::tui::effect::effect::Effect::ReplyInteraction {
            reply: crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(answers),
            ..
        } if answers == &vec!["乙".to_string()]
    )));
    harness.assert_idle();
}
