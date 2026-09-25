//! AskUserQuestion Type something 子态的粘贴投递场景（paste target 决策）。
//!
//! 粘贴与 Cmd+V 按键共用 `route_paste` 的目标决策：子态进自由输入框，
//! AskUser 活动但非子态时忽略，否则走主输入区。

use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::adapter::tui_runtime_event::{
    TuiInteractionBody, TuiInteractionRequest, TuiOptionItem, TuiRuntimeEvent, TuiUserQuestion,
};
use crate::tui::model::conversation::interaction::UiInteractionRequestId;

use super::super::testing::{input, ExpectedEffect, TuiScenarioHarness};

fn request_with_options(request_id: &str) -> TuiRuntimeEvent {
    TuiRuntimeEvent::InteractionRequested(TuiInteractionRequest {
        request_id: UiInteractionRequestId::from(request_id),
        run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
        tool_call_id: None,
        body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
            prompt: "问题".to_string(),
            options: vec![TuiOptionItem {
                title: "A".to_string(),
                description: Some("A的描述".to_string()),
            }],
            allow_multi: false,
        }]),
    })
}

fn request_without_options(request_id: &str) -> TuiRuntimeEvent {
    TuiRuntimeEvent::InteractionRequested(TuiInteractionRequest {
        request_id: UiInteractionRequestId::from(request_id),
        run_id: crate::tui::model::conversation::interaction::UiRunId::from("run-1"),
        tool_call_id: None,
        body: TuiInteractionBody::UserQuestions(vec![TuiUserQuestion {
            prompt: "自由输入问题".to_string(),
            options: vec![],
            allow_multi: false,
        }]),
    })
}

#[test]
fn paste_during_free_input_targets_ask_box_not_main_input() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    // 无 LLM 选项 → Enter 直接进入 Type something 子态
    harness.runtime_event(request_without_options("test-paste-free"));
    harness.render();
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        harness
            .app
            .model
            .conversation
            .ask_user_snapshot()
            .expect("substate active")
            .chat_input_active
    );

    harness.paste("pasted content");

    assert_eq!(
        harness
            .app
            .model
            .conversation
            .ask_user_chat_text()
            .as_deref(),
        Some("pasted content"),
        "粘贴文本必须进入 Type something 自由输入框"
    );
    assert_eq!(harness.input_text(), "", "粘贴文本不得落入主输入区");
    harness.assert_idle();
}

#[test]
fn paste_on_question_options_does_not_pollute_main_input() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    harness.runtime_event(request_with_options("test-paste-options"));
    harness.render();

    // 选项列表子态外（AskUser 活动中）粘贴必须被忽略，不污染主输入区
    harness.paste("junk text");

    assert_eq!(
        harness.input_text(),
        "",
        "AskUser 活动中粘贴不得进入主输入区"
    );
    assert!(
        harness.app.model.conversation.ask_user_snapshot().is_some(),
        "AskUser batch 必须保持可交互"
    );
    harness.assert_idle();
}

#[test]
fn super_v_during_free_input_reads_clipboard_for_ask_box() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();
    harness.runtime_event(request_without_options("test-super-v"));
    harness.render();
    harness.key(input::press(KeyCode::Enter, KeyModifiers::NONE));

    // 子态 Cmd+V / Ctrl+V 必须读取剪贴板（与主输入区同一决策入口）
    harness.expect_effect(ExpectedEffect::ReadClipboardImage);
    harness.key(input::press(KeyCode::Char('v'), KeyModifiers::SUPER));
    assert!(
        harness.effects().iter().any(|effect| matches!(
            effect,
            crate::tui::effect::effect::Effect::ReadClipboardImage
        )),
        "子态 Cmd+V 必须触发剪贴板读取"
    );

    // executor 回灌 paste 事件 → 文本进自由输入框
    harness.paste("clipboard text");
    assert_eq!(
        harness
            .app
            .model
            .conversation
            .ask_user_chat_text()
            .as_deref(),
        Some("clipboard text"),
        "剪贴板文本必须进入自由输入框"
    );
    assert_eq!(harness.input_text(), "", "剪贴板文本不得落入主输入区");
    harness.assert_idle();
}

#[test]
fn paste_without_ask_user_still_targets_main_input() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.chat.start_processing();

    harness.paste("normal paste");

    assert_eq!(
        harness.input_text(),
        "normal paste",
        "无 AskUser 交互时粘贴保持进入主输入区"
    );
    harness.assert_idle();
}
