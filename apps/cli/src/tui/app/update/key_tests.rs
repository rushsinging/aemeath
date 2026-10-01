use super::*;
use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::interaction::UiQueuedInputId;
use crate::tui::model::input::completion::SuggestionType;
use crate::tui::model::input::completion_item::CompletionItem;
use crate::tui::update::msg::TuiMsg;

fn make_spawn_refs() -> SpawnContextRefs {
    SpawnContextRefs { agent_client: None }
}

/// 构造 live main root activity（purpose 可参），注入 conversation 的 activity 镜像，
/// 模拟 Runtime 权威快照中 Main / Manual Reflection Run 正在执行的事实。
fn inject_live_main_root(
    app: &mut App,
    run_id: &str,
    purpose: crate::tui::adapter::tui_runtime_event::TuiRunPurpose,
) {
    use crate::tui::adapter::tui_runtime_event::*;
    use crate::tui::model::conversation::interaction::UiRunId;
    let root = TuiActivityObservation {
        id: UiActivityId::from("root"),
        run_id: UiRunId::from(run_id),
        run_step_id: None,
        parent_activity_id: None,
        source: TuiActivitySource::Run,
        kind: TuiActivityKind::Run,
        state: TuiActivityState::Running,
        detail: TuiActivityDetail::Run { purpose },
        audience: TuiActivityAudience::User,
        revision: 1,
        timing: TuiActivityTiming::default(),
    };
    app.model
        .conversation
        .activity_observations_mut()
        .replace_for_test(UiRunId::from(run_id), 1, vec![root]);
}

/// Manual Reflection Run 不产生 RunStep（`active_run_step` 为 None）且 slash 事件不开
/// turn（`is_processing` 为 false）：Esc 必须经 `CancelCurrentRun` 让 Runtime 裁决取消，
/// NEVER 静默无效。
#[test]
fn esc_during_live_reflection_run_requests_current_run_cancel() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    inject_live_main_root(
        &mut app,
        "reflection-run",
        crate::tui::adapter::tui_runtime_event::TuiRunPurpose::Reflection,
    );
    let spawn_refs = make_spawn_refs();

    let result = app.update_key(
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &spawn_refs,
    );

    assert_eq!(result.effects, vec![Effect::CancelCurrentRun]);
}

/// Ctrl-C 与 Esc 同一取消面：reflection 运行期间视为有活跃执行，优先 RequestCancel
/// 而非 ClearInput / WarnExit。
#[test]
fn ctrl_c_during_live_reflection_run_requests_current_run_cancel() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    inject_live_main_root(
        &mut app,
        "reflection-run",
        crate::tui::adapter::tui_runtime_event::TuiRunPurpose::Reflection,
    );
    let spawn_refs = make_spawn_refs();

    let result = app.update_key(
        crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &spawn_refs,
    );

    assert_eq!(result.effects, vec![Effect::CancelCurrentRun]);
}

/// Main Run 的 step 间隙（`is_processing` 仍为 true、live Main root 存在）：Esc
/// 发 `CancelCurrentRun`，由 Runtime 裁决（间隙无执行单元 → NoActiveStep 提示），
/// TUI 不自行判断间隙语义。
#[test]
fn esc_in_step_gap_of_processing_run_requests_current_run_cancel() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    inject_live_main_root(
        &mut app,
        "main-run",
        crate::tui::adapter::tui_runtime_event::TuiRunPurpose::Main,
    );
    let spawn_refs = make_spawn_refs();

    let result = app.update_key(
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &spawn_refs,
    );

    assert_eq!(result.effects, vec![Effect::CancelCurrentRun]);
}

/// 有 live Main root 且 step 执行中（processing）：Esc 同样发 `CancelCurrentRun`——
/// 统一入口不区分 step 是否可寻址，registry 内部有 step 走 CancelStep 协议。
#[test]
fn esc_with_live_main_root_sends_cancel_current_run() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    inject_live_main_root(
        &mut app,
        "run-1",
        crate::tui::adapter::tui_runtime_event::TuiRunPurpose::Main,
    );
    let spawn_refs = make_spawn_refs();

    let result = app.update_key(
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &spawn_refs,
    );

    assert_eq!(result.effects, vec![Effect::CancelCurrentRun]);
}

/// 忙碌时 slash 仍必须交给统一 CommandRouter/handler，不能压成 Runtime 无法执行的
/// `ControlCommand`。否则 `/compact` 会在 busy gate 后被静默丢弃。
#[test]
fn busy_slash_dispatches_synchronously_without_placeholder() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    app.model
        .input
        .apply(InputIntent::InsertPastedText("/compact".to_string()));
    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

    let result = app.update_key(key, &spawn_refs);

    // busy slash 同步分发：App::new 注入真实 builtin router，/compact 产出
    // Compact 事件 effect；不降级为 ControlCommand，也不建占位 QueuedUserMessage。
    assert!(
        result.effects.iter().any(|effect| matches!(
            effect,
            Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::Compact
            }
        )),
        "busy slash 应同步产出 Compact 事件 effect，实际: {:?}",
        result.effects
    );
    assert!(
        app.model.conversation.queued_submissions.is_empty(),
        "busy slash 后不应建占位 QueuedUserMessage"
    );
    // #1816：命令占位改由 runtime 的 ControlCommandsQueued 权威快照驱动，
    // 提交瞬间 TUI 侧刻意不建占位——避免与 runtime 快照双轨、也避免假回显。
    assert!(
        app.model.conversation.queued_commands.is_empty(),
        "命令占位必须等 runtime 快照，NEVER 由 TUI 乐观创建"
    );
}

/// processing（turn 进行中）时 Esc / Ctrl-C 统一发 `CancelCurrentRun`（无 identity，
/// Runtime 控制面裁决当前执行单元：有 step 走 step 取消协议，无 step 按 intent
/// 分流）。TUI 不再持有 run/step identity 取消面。
#[test]
fn esc_and_ctrl_c_during_processing_send_cancel_current_run() {
    let mut esc_app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    esc_app.chat.start_processing();
    let spawn_refs = SpawnContextRefs { agent_client: None };
    let esc = esc_app.update_key(
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &spawn_refs,
    );

    let mut ctrl_c_app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    ctrl_c_app.chat.start_processing();
    let ctrl_c = ctrl_c_app.update_key(
        crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &spawn_refs,
    );

    assert_eq!(esc.effects, vec![Effect::CancelCurrentRun]);
    assert_eq!(ctrl_c.effects, vec![Effect::CancelCurrentRun]);
}

/// 无活跃执行（非 processing 且无 live main root）时 Esc / Ctrl-C 不发取消：
/// idle 的 Ctrl-C 走 ClearInput / 双击 terminate（退出）路径，Esc 无效果。
#[test]
fn esc_and_ctrl_c_without_active_execution_send_no_cancel() {
    let mut esc_app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    let spawn_refs = SpawnContextRefs { agent_client: None };
    let esc = esc_app.update_key(
        crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        &spawn_refs,
    );

    let mut ctrl_c_app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    let ctrl_c = ctrl_c_app.update_key(
        crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &spawn_refs,
    );

    assert!(
        !esc.effects
            .iter()
            .any(|effect| matches!(effect, Effect::CancelCurrentRun)),
        "无活跃执行时 Esc 不得发取消: {:?}",
        esc.effects
    );
    assert!(
        !ctrl_c
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::CancelCurrentRun)),
        "无活跃执行时 Ctrl-C 不得发取消（走清空/退出路径）: {:?}",
        ctrl_c.effects
    );
}

#[test]
fn cancel_command_ack_does_not_publish_or_apply_terminal() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    let timeline_before_ack = app.model.conversation.timeline.items().len();
    app.chat.start_cancelling();

    assert!(app.chat.is_processing, "ACK 不能结束 processing");
    assert!(
        app.chat.is_cancelling,
        "accepted ACK 只进入 cancelling 展示态"
    );
    assert_eq!(
        app.model.conversation.timeline.items().len(),
        timeline_before_ack,
        "ACK 不能伪造 Runtime terminal"
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
fn test_ctrlc_action_cancelling_second_press_requests_cancel_again() {
    assert_eq!(
        ctrlc_action(true, None, true, true),
        CtrlCAction::RequestCancel
    );
    assert_eq!(
        ctrlc_action(false, None, true, true),
        CtrlCAction::RequestCancel
    );
}

/// 忙时提交含多字节 UTF-8 字符的长文本时，日志预览截断不得在字符边界内
/// 用字节索引切片 panic（59 字节 ASCII 后接「任」，60 字节截断点落在字符中间）。
#[test]
fn test_busy_enter_multibyte_long_text_does_not_panic() {
    // log_debug! 在级别未达 Debug 时不求值参数，必须显式打开才能让
    // mid_turn.enter 的预览截断代码真正执行。
    log::set_max_level(log::LevelFilter::Debug);
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    // 59 个 ASCII + 1 个「任」：字符区间 59..62，字节截断点 60 落在字符中间。
    let long_text = format!("{}任", "x".repeat(59));
    app.model
        .input
        .apply(InputIntent::InsertPastedText(long_text.clone()));
    let spawn_refs = SpawnContextRefs { agent_client: None };
    let key = crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

    let result = app.update_key(key, &spawn_refs);

    assert!(matches!(
        result.effects.as_slice(),
        [Effect::SendChatInputEvent {
            event: sdk::ChatInputEvent::UserMessage { text, .. }
        }] if text == &long_text
    ));
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

/// TaskData 5 (A3) — Up 键走光标/历史导航，不清除占位区。
#[test]
fn test_up_arrow_busy_with_queued_sends_withdraw_all() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    // 入队两条占位（模拟忙时提交）
    app.enqueue_submission_echo("key-1", "first");
    app.enqueue_submission_echo("key-2", "second");

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

/// #1816：排队里只有控制命令时，Up 键同样撤回（此前命令完全撤不掉）。
#[test]
fn test_up_arrow_busy_with_queued_command_only_sends_withdraw_all() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel(4);
    let spawn_refs = make_spawn_refs();
    app.update(
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::ControlCommandsQueued {
            queued: vec![(
                UiQueuedInputId::from("01920000-0000-7000-8000-000000000001"),
                "/compact".to_string(),
            )],
        }]),
        &ui_tx,
        &spawn_refs,
    );

    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
    let result = app.update_key(key, &spawn_refs);

    let has_withdraw = result.effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::SendChatInputEvent {
                event: sdk::ChatInputEvent::WithdrawAll
            }
        )
    });
    assert!(has_withdraw, "只有排队命令时 Up 键也必须能撤回（#1816）");
    assert!(
        app.model.conversation.queued_commands.is_empty(),
        "Up 键乐观清空命令占位"
    );
    assert_eq!(
        app.model.input.document.display_text(),
        "/compact",
        "撤回的命令文本应还原到输入框，用户可编辑后重新提交"
    );
}

/// #1816：消息与命令混排时按入队序号合并还原。
#[test]
fn test_up_arrow_restores_messages_and_commands_in_arrival_order() {
    let mut app = App::new(
        "test-session".to_string(),
        std::path::PathBuf::from("/tmp"),
        "test-model".to_string(),
    );
    app.chat.start_processing();
    app.enqueue_submission_echo("01920000-0000-7000-8000-000000000001", "先到的消息");
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel(4);
    let spawn_refs = make_spawn_refs();
    app.update(
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::ControlCommandsQueued {
            queued: vec![(
                UiQueuedInputId::from("01920000-0000-7000-8000-000000000002"),
                "/compact".to_string(),
            )],
        }]),
        &ui_tx,
        &spawn_refs,
    );

    let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
    app.update_key(key, &spawn_refs);

    assert_eq!(
        app.model.input.document.display_text(),
        "先到的消息\n/compact",
        "还原顺序必须等于提交顺序（#1816）"
    );
}
