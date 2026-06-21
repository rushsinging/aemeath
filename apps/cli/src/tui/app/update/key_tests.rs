use super::*;
use crate::tui::effect::effect::Effect;

/// Task 5 (A3) — busy + slash 提交：产 ControlCommand 事件，不建占位。
#[test]
fn test_busy_slash_no_placeholder() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    app.model
        .input
        .apply(InputIntent::InsertPastedText("/foo bar".to_string()));
    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

    let result = app.update_key(key, &spawn_refs);

    // 应产出 ControlCommand 事件
    assert!(
        matches!(
            result.effects.as_slice(),
            [Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::ControlCommand { raw }
            }] if raw == "/foo bar"
        ),
        "busy-slash 应产出 ControlCommand 事件，got: {:?}",
        result.effects
    );
    // conversation 中不应有 QueuedUserMessage 块
    assert!(
        app.model.conversation.queued_submissions.is_empty(),
        "busy-slash 后不应建占位 QueuedUserMessage"
    );
}

#[test]
fn test_ctrlc_action_input_nonempty_clears() {
    assert_eq!(
        ctrlc_action(false, None, false, false),
        CtrlCAction::ClearInput
    );
    assert_eq!(
        ctrlc_action(false, Some(std::time::Instant::now()), false, false),
        CtrlCAction::ClearInput
    );
}

#[test]
fn test_ctrlc_action_empty_first_press_warns() {
    assert_eq!(
        ctrlc_action(true, None, false, false),
        CtrlCAction::WarnExit
    );
}

#[test]
fn test_ctrlc_action_empty_quick_second_press_quits() {
    let recent = std::time::Instant::now();
    assert_eq!(
        ctrlc_action(true, Some(recent), false, false),
        CtrlCAction::Quit
    );
}

#[test]
fn test_ctrlc_action_empty_expired_second_press_warns() {
    let expired = std::time::Instant::now() - std::time::Duration::from_secs(4);
    assert_eq!(
        ctrlc_action(true, Some(expired), false, false),
        CtrlCAction::WarnExit
    );
}

#[test]
fn test_ctrlc_action_boundary_timeout() {
    let just_inside = std::time::Instant::now() - std::time::Duration::from_millis(2900);
    assert_eq!(
        ctrlc_action(true, Some(just_inside), false, false),
        CtrlCAction::Quit
    );

    let just_outside = std::time::Instant::now() - std::time::Duration::from_millis(3100);
    assert_eq!(
        ctrlc_action(true, Some(just_outside), false, false),
        CtrlCAction::WarnExit
    );
}

#[test]
fn test_ctrlc_action_processing_first_press_requests_cancel() {
    assert_eq!(
        ctrlc_action(true, None, true, false),
        CtrlCAction::RequestCancel
    );
    assert_eq!(
        ctrlc_action(false, None, true, false),
        CtrlCAction::RequestCancel
    );
}

#[test]
fn test_ctrlc_action_cancelling_second_press_force_quits() {
    assert_eq!(ctrlc_action(true, None, true, true), CtrlCAction::ForceQuit);
    assert_eq!(
        ctrlc_action(false, None, true, true),
        CtrlCAction::ForceQuit
    );
}

#[test]
fn test_update_key_queued_copied_text_sends_original_and_previews_placeholder() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    app.model
        .input
        .apply(InputIntent::InsertPastedText("a\nb\nc\nd".to_string()));
    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

    let result = app.update_key(key, &spawn_refs);

    assert_eq!(
        app.live_status_view_model().queued_lines,
        vec!["> [Copied 4 lines]"]
    );
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::SendChatInputEvent {
            event: sdk::ChatInputEvent::UserMessage { text, .. }
        }] if text == "a\nb\nc\nd"
    ));
}

/// Task 5 (A3) — Up 键走光标/历史导航，不清除占位区。
#[test]
fn test_up_arrow_busy_with_queued_sends_withdraw_all() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    // 入队两条占位（模拟忙时提交）
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "first");
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "second");

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);

    let result = app.update_key(key, &spawn_refs);

    // #391 S3-5：busy + 有 queued → Up 键发 WithdrawAll（runtime gate 批量撤回）。
    let has_withdraw = result.effects.iter().any(|e| {
        matches!(
            e,
            Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::WithdrawAll
            }
        )
    });
    assert!(has_withdraw, "busy + 有 queued 时 Up 键应发 WithdrawAll");
    // queued_submissions 不在此同步清理（等 runtime 回传 UserMessagesWithdrawn 后清）。
    assert_eq!(
        app.model.conversation.queued_submissions.len(),
        2,
        "Up 键自身不清 queued_submissions（由 UiEvent::UserMessagesWithdrawn handler 清）"
    );
}

#[test]
fn test_up_arrow_idle_or_no_queued_moves_cursor() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    // idle 态（未 start_processing）→ MoveCursorUp，不发 WithdrawAll
    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
    let result = app.update_key(key, &spawn_refs);
    let has_withdraw = result.effects.iter().any(|e| {
        matches!(
            e,
            Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::WithdrawAll
            }
        )
    });
    assert!(!has_withdraw, "idle 态 Up 键不应发 WithdrawAll");

    // busy 但无 queued → 也不发 WithdrawAll
    app.chat.start_processing();
    let result2 = app.update_key(key, &spawn_refs);
    let has_withdraw2 = result2.effects.iter().any(|e| {
        matches!(
            e,
            Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::WithdrawAll
            }
        )
    });
    assert!(
        !has_withdraw2,
        "busy 但无 queued 时 Up 键不应发 WithdrawAll"
    );
}

#[test]
fn test_up_arrow_history_recall() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    // 设置 history
    app.model
        .input
        .apply(InputIntent::ReplaceHistory(vec!["past input".to_string()]));

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    // Up 键走 history recall
    assert_eq!(app.model.input.document.buffer, "past input");
}
