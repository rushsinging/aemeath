use super::*;
use crate::tui::effect::effect::Effect;
use crate::tui::model::input::completion::SuggestionType;
use crate::tui::model::input::completion_item::CompletionItem;

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
    // #589: Up 键乐观清空 queued_submissions，不等 runtime round-trip。
    assert_eq!(
        app.model.conversation.queued_submissions.len(),
        0,
        "Up 键乐观清空 queued_submissions（#589 即时撤回）"
    );
    // 还原输入框：两条 queued 文本 join("\n") 后回填到 input buffer。
    assert_eq!(
        app.model.input.document.display_text(),
        "first\nsecond",
        "Up 键撤回后 queued 文本应还原到输入框"
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
fn test_busy_slash_triggers_completion() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    assert_eq!(app.model.input.document.buffer, "/");
    assert!(app.model.input.completion.visible);
    assert_eq!(app.model.input.completion.query, "/");
    assert!(app
        .model
        .input
        .completion
        .items
        .iter()
        .any(|item| item.label == "/help"));
}

#[test]
fn test_busy_at_triggers_mention_completion_state() {
    let cwd = std::env::temp_dir().join(format!("aemeath-key-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).expect("create temp cwd");
    std::fs::write(cwd.join("src.rs"), "").expect("write mention candidate");
    let mut app = App::new(
        "test-session".to_string(),
        cwd.clone(),
        "test-model".to_string(),
    );
    app.chat.start_processing();

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Char('@'), KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    assert_eq!(app.model.input.document.buffer, "@");
    assert!(app.model.input.completion.visible);
    assert_eq!(app.model.input.completion.query, "@");
    assert!(app
        .model
        .input
        .completion
        .items
        .iter()
        .any(|item| item.label == "src.rs"));
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn test_busy_backspace_refreshes_completion() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    app.model
        .input
        .apply(InputIntent::ReplaceText("/he".to_string()));
    app.handle_input_intent(InputIntent::SetCompletions {
        query: "/he".to_string(),
        items: vec![CompletionItem::new("/help", "/help")],
    });

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    assert_eq!(app.model.input.document.buffer, "/h");
    assert!(app.model.input.completion.visible);
    assert_eq!(app.model.input.completion.query, "/h");
    assert!(app
        .model
        .input
        .completion
        .items
        .iter()
        .any(|item| item.label == "/help"));
}

#[test]
fn test_busy_esc_closes_completion_before_interrupting_runtime() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    app.handle_input_intent(InputIntent::SetCompletions {
        query: "/".to_string(),
        items: vec![CompletionItem::new("/help", "/help")],
    });

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    assert!(!app.model.input.completion.visible);
    assert!(app.chat.is_processing);
    assert!(app
        .model
        .conversation
        .runtime
        .transient_notice_expiry
        .is_none());
}

#[test]
fn test_busy_tab_applies_visible_completion() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    app.model
        .input
        .apply(InputIntent::ReplaceText("/he".to_string()));
    app.handle_input_intent(InputIntent::SetCompletions {
        query: "/he".to_string(),
        items: vec![CompletionItem::with_type(
            "/help",
            "/help",
            SuggestionType::Command,
        )],
    });

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    assert_eq!(app.model.input.document.buffer, "/help");
    assert!(!app.model.input.completion.visible);
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
