use super::*;

/// Task 5 (A3) — busy + slash 提交：产 ControlCommand 事件，不建占位，input_queue 为空。
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
    // input_queue 应为空——slash 命令不入文本队列
    assert_eq!(app.input.queue_len(), 0, "busy-slash 后 input_queue 应为空");
    // conversation 中不应有 QueuedUserMessage 块
    assert!(
        app.model.conversation.queued_submissions.is_empty(),
        "busy-slash 后不应建占位 QueuedUserMessage"
    );
}

/// Task 5 (A3) — Up 键：input_queue 非空时不再恢复文本（光标/历史导航）。
#[test]
fn test_up_arrow_no_queue_restore_when_queue_nonempty() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    // 手动往 queue 放一条（模拟旧路径遗留，或其他来源）
    app.input.push_queue("queued-text".to_string());
    // input area 为空
    assert!(app.model.input.document.is_empty());

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    // input area 不应被恢复为 queue 内容
    assert!(
        app.model.input.document.is_empty(),
        "Up 键不应再将 queue 内容恢复到 input area，got: {:?}",
        app.model.input.document.buffer
    );
    // queue 不应被清空（Up 键不再触发 drain）
    assert_eq!(app.input.queue_len(), 1, "Up 键不应 drain input_queue");
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

    // A3 Task 1：忙时提交文本统一走事件通道，不再双写 input_queue（#390 A3 Task 1）。
    assert_eq!(
        app.input.queue_len(),
        0,
        "忙时 submit 后 input_queue 应为空"
    );
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

/// Task 5 (A3) — Up 键队列召回分支已删除：队列非空时 Up 键走光标/历史导航，不恢复队列内容。
/// 原测试断言 Up 键恢复 queue，现已翻转为新行为。
#[test]
fn test_up_arrow_does_not_restore_queued_input_to_input_area() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    // 入队两条消息（模拟旧路径写入，Up 键不再召回）
    app.input.push_queue("first".to_string());
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "first");
    app.input.push_queue("second".to_string());
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "second");

    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);

    let _ = app.update_key(key, &spawn_refs);

    // Task 5: queue 不应被清空（Up 键不再 drain queue）
    assert_eq!(app.input.queue_len(), 2, "Up 键不应 drain input_queue");
    // input area 不应被恢复
    assert!(
        app.model.input.document.is_empty(),
        "Up 键不应恢复 queue 内容到 input area"
    );
    // queued_submissions 不应被清除（Up 键不再触发 clear_queued_submission_echo）
    assert_eq!(
        app.model.conversation.queued_submissions.len(),
        2,
        "Up 键不应清除 queued_submissions"
    );
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
