use super::*;

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

    assert_eq!(app.input.queue_preview(), "a\nb\nc\nd");
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

#[test]
fn test_up_arrow_restores_all_queued_input_to_input_area() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    // 入队两条消息
    app.input.push_queue("first".to_string());
    app.enqueue_submission_echo("first");
    app.input.push_queue("second".to_string());
    app.enqueue_submission_echo("second");

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    // queue 应被清空
    assert_eq!(app.input.queue_len(), 0);
    // input area 应包含两条排队内容（\n 连接）
    assert_eq!(app.model.input.document.buffer, "first\nsecond");
    // 排队显示块应被清除
    assert!(app.model.conversation.queued_submissions.is_empty());
}

#[test]
fn test_up_arrow_history_recall_when_queue_empty() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    // 设置 history
    app.model
        .input
        .apply(InputIntent::ReplaceHistory(vec!["past input".to_string()]));
    // queue 为空
    assert_eq!(app.input.queue_len(), 0);

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    // queue 空时应走 history recall
    assert_eq!(app.model.input.document.buffer, "past input");
}
