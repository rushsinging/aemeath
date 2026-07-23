use super::*;
use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock, TuiMessageSource};
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
use std::path::PathBuf;

fn make_spawn_refs() -> SpawnContextRefs {
    SpawnContextRefs { agent_client: None }
}

fn test_app() -> App {
    App::new(
        "test-session".to_string(),
        PathBuf::from("/tmp"),
        "test-model".to_string(),
    )
}

/// 消息同步事件（如 PostToolExecutionSync）只镜像 chat.messages，不产生 UserMessage 回显块，
/// 也不清除占位（回显与占位清理由 UserMessagesAdopted 负责）。
#[test]
fn test_update_ui_post_tool_sync_only_mirrors_no_echo() {
    let mut app = test_app();
    let echo_id = "echo-1".to_string();
    app.enqueue_submission_echo(echo_id, "[Copied Text 1]");
    let messages = vec![
        TuiChatMessage::user_text("first"),
        TuiChatMessage::user_text("a\nb\nc"),
    ];
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = SpawnContextRefs { agent_client: None };

    app.update_ui(
        UiEvent::PostToolExecutionSync { messages },
        &ui_tx,
        &spawn_refs,
    );

    // chat.messages 已删除，不再断言镜像数量

    // 不产生任何 UserMessage 回显块（退出 display）
    assert!(app.model.conversation.timeline.items().iter().all(|item| {
        !matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. } if text == "a\nb\nc")
    }));

    // 占位未被清除（归 UserMessagesAdopted 负责）
    assert_eq!(
        app.model.conversation.queued_submissions.len(),
        1,
        "消息同步不应清占位"
    );
}

#[test]
fn test_update_ui_post_tool_sync_does_not_echo_system_generated_user_message() {
    let mut app = test_app();
    let reminder = "<system-reminder>\nStop hook blocked stopping.\n</system-reminder>";
    let messages = vec![
        TuiChatMessage::user_text("first"),
        TuiChatMessage::system_generated_user_text(reminder),
    ];
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = SpawnContextRefs { agent_client: None };

    app.update_ui(
        UiEvent::PostToolExecutionSync { messages },
        &ui_tx,
        &spawn_refs,
    );

    assert!(app.model.conversation.timeline.items().iter().all(|item| {
        !matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. } if text == reminder)
    }));
}

/// 消息同步事件只镜像 + 落盘，不产生 display
///
/// 场景：存在一条占位（id_a="hello"），收到包含 user_text("hello") 的同步事件。
/// 期望：
/// - handler 后 PostToolExecutionSync 不再镜像 chat.messages（字段已删除）
/// - 不产生任何 UserMessage 回显块（退出 display）
/// - 占位未被清除（清占位归 UserMessagesAdopted 负责）
#[test]
fn test_post_tool_sync_no_display() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    // 入队一条占位
    let id_a = "input-a".to_string();
    app.enqueue_submission_echo(id_a, "hello");
    assert_eq!(app.model.conversation.queued_submissions.len(), 1);

    // 构造包含该 user message 的 msgs
    let msgs = vec![TuiChatMessage::user_text("hello")];
    app.update_ui(
        UiEvent::PostToolExecutionSync {
            messages: msgs.clone(),
        },
        &ui_tx,
        &spawn_refs,
    );

    // PostToolExecutionSync 不再镜像 chat.messages（字段已删除）
    // 不产生 UserMessage 回显块

    // 不产生 UserMessage 回显块
    let user_echo_count = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter(|b| {
            matches!(
                b,
                crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { .. }
            )
        })
        .count();
    assert_eq!(
        user_echo_count, 0,
        "MessagesSync 不应产生 UserMessage 回显块（退出 display）"
    );

    // 占位未被清除（应由 UserMessagesAdopted 负责）
    assert_eq!(
        app.model.conversation.queued_submissions.len(),
        1,
        "MessagesSync 不应清除占位（清占位归 UserMessagesAdopted）"
    );
}

/// Task 3: UserMessagesAdopted 按 id 清占位 + 顺序回显
///
/// 场景：入队两条占位（A="hi"，B="yo"）；
/// handler 收到 UserMessagesAdopted([{id:A,"hi"},{id:B,"yo"}])
/// → A/B 占位全清、按序追加两条正式 UserMessage 回显 "hi"/"yo"，无残留占位。
#[test]
fn test_user_messages_added_consumes_placeholders_and_echoes_in_order() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    // 入队两条占位（id_a / id_b）
    let id_a = "input-a".to_string();
    let id_b = "input-b".to_string();
    app.enqueue_submission_echo(id_a.clone(), "hi");
    app.enqueue_submission_echo(id_b.clone(), "yo");

    // 确认两条占位已在 model 中
    assert_eq!(app.model.conversation.queued_submissions.len(), 2);

    // 触发 handler
    let items = vec![
        TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text("hi")],
            input_id: Some(id_a.clone()),
            source: TuiMessageSource::User,
            stop_hook: None,
        },
        TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text("yo")],
            input_id: Some(id_b.clone()),
            source: TuiMessageSource::User,
            stop_hook: None,
        },
    ];
    app.update_ui(
        UiEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        },
        &ui_tx,
        &spawn_refs,
    );

    // 占位全清
    assert!(
        app.model.conversation.queued_submissions.is_empty(),
        "handler 执行后不应有残留占位"
    );
    let queued_blocks = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter(|b| {
            matches!(
                b,
                crate::tui::model::output_timeline::OutputTimelineItem::QueuedUserMessage { .. }
            )
        })
        .count();
    assert_eq!(queued_blocks, 0, "不应有残留 QueuedUserMessage 块");

    // 按序追加两条正式 UserMessage
    let user_echo_texts: Vec<&str> = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|b| {
            if let crate::tui::model::output_timeline::OutputTimelineItem::UserMessage {
                text,
                ..
            } = b
            {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        user_echo_texts,
        vec!["hi", "yo"],
        "应按序追加两条正式 UserMessage 回显"
    );
}

/// #507 回归：UserMessagesAdopted 携带 ChatMessage（typed blocks 含 Image.placeholder）
/// 时，回显文本应经 message.text_content() 还原出用户视角完整文本（含占位符）。
///
/// 场景：用户输入"看图[Image #1]"（TUI 端 enqueue_submission_echo 用 display_text
/// 写入排队块）；runtime 端构造 ChatMessage（content 含 Image { placeholder } + 对应
/// input_id），通过 UserMessagesAdopted 携带。
/// handler 收到后：
/// - 按 message.input_id 清除对应占位块
/// - 用 message.text_content() 还原 "看图[Image #1]"，写入 UserMessage 回显
#[test]
fn test_user_messages_added_echoes_image_placeholder_from_message() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    // 用户提交"看图[Image #1]"——TUI 端 enqueue 占位（display_text 含占位符）
    let input_id = "image-input".to_string();
    app.enqueue_submission_echo(input_id.clone(), "看图[Image #1]");
    assert_eq!(app.model.conversation.queued_submissions.len(), 1);

    // runtime 端构造的 ChatMessage：image block 携带 placeholder（用于 text_content 还原位置）
    let items = vec![TuiChatMessage {
        role: "user".to_string(),
        content: vec![
            TuiContentBlock::text("看图"),
            TuiContentBlock::Image {
                media_type: "image/png".to_string(),
                base64: "aW1nZGF0YQ==".to_string(),
                placeholder: Some("[Image #1]".to_string()),
            },
        ],
        input_id: Some(input_id.clone()),
        source: TuiMessageSource::User,
        stop_hook: None,
    }];

    app.update_ui(
        UiEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        },
        &ui_tx,
        &spawn_refs,
    );

    // 占位被清除
    assert!(
        app.model.conversation.queued_submissions.is_empty(),
        "handler 应按 input_id 清占位"
    );

    // 回显文本应含占位符（"看图[Image #1]"）——这是 #507 修复目标
    let user_echo_texts: Vec<&str> = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|b| {
            if let crate::tui::model::output_timeline::OutputTimelineItem::UserMessage {
                text,
                ..
            } = b
            {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        user_echo_texts,
        vec!["看图[Image #1]"],
        "回显应经 message.text_content() 还原含占位符（#507 修复目标）"
    );
}

/// Bug #540：MessagesSync 兜底清理必须同时清空 compact runtime 三态（chat_active、
/// phase、running_tool_count、compact_progress），否则 compact 完成后 spinner 行会
/// 残留 Compacting 文案 + 90% 进度条。
#[test]
fn test_messages_sync_clears_compact_runtime_state() {
    use crate::tui::model::conversation::intent::SetCompactProgress;
    use crate::tui::model::conversation::spinner::SpinnerPhase;

    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    // 模拟 compact 进行中：直接写入 runtime 三态
    app.model.conversation.runtime.spinner.chat_active = true;
    app.model.conversation.runtime.spinner.phase = Some(SpinnerPhase::Compacting);
    app.model.conversation.runtime.spinner.running_tool_count = 2;
    app.model.conversation.apply(SetCompactProgress {
        stage: "finalizing".into(),
        current: Some(8),
        total: Some(10),
    });
    assert!(
        app.model.conversation.runtime.compact_progress.is_some(),
        "precondition: compact_progress 已设置"
    );

    app.update_ui(
        UiEvent::CompactFinished { messages: vec![] },
        &ui_tx,
        &spawn_refs,
    );

    // CompactFinished 后：compact runtime 状态被清空，但 spinner 不停（turn 仍在进行）
    assert!(
        app.model.conversation.runtime.compact_progress.is_none(),
        "CompactFinished 后 compact_progress 必须清空"
    );
    // spinner 不应被 CompactFinished 停止
    assert!(
        app.model.conversation.runtime.spinner.chat_active,
        "CompactFinished 后 chat_active 保持 true（turn 仍在进行）"
    );
    assert!(
        app.model.conversation.runtime.compact_progress.is_none(),
        "MessagesSync 后 compact_progress 必须清空（进度条才会消失）"
    );
    assert!(
        app.view_state.dirty.output,
        "MessagesSync 必须 mark_output_dirty 触发进度条消失渲染"
    );
}

/// #749：ApiError 退化为纯展示 —— 追加一次错误 notice，NOT 自行清 processing
/// （收口统一交给随后的 DoneWithDuration）。
#[test]
fn test_api_error_appends_notice_and_defers_processing_to_done() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    // 模拟 turn 进行中
    app.chat.start_processing();
    assert!(app.chat.is_processing);

    let error = "stream error: stream interrupted after partial output".to_string();
    app.update_ui(
        UiEvent::ApiError {
            messages: vec![],
            error: error.clone(),
        },
        &ui_tx,
        &spawn_refs,
    );

    // 错误 notice 已注入（供用户可见），且只出现一次
    let error_hits = system_notice_texts(&app)
        .iter()
        .filter(|t| t.contains("stream interrupted after partial output"))
        .count();
    assert_eq!(error_hits, 1, "ApiError 应追加恰好一次错误 notice");

    // ApiError 本身不清 processing —— 收口交给 DoneWithDuration
    assert!(
        app.chat.is_processing,
        "ApiError 不应自行清 processing，收口交给 Done"
    );
}

/// #749 核心回归：API 错误 turn 终止序列（ApiError → DoneWithDuration）后，
/// is_processing 必须回到 false，下一条输入才能正常开启新 turn（不进 queue）。
#[test]
fn test_api_error_then_done_clears_processing() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    app.chat.start_processing();
    assert!(app.chat.is_processing);

    // runtime 端 API 错误路径：先 ApiError 后 DoneWithDuration。
    app.update_ui(
        UiEvent::ApiError {
            messages: vec![],
            error: "stream error: boom".to_string(),
        },
        &ui_tx,
        &spawn_refs,
    );
    app.update_ui(
        UiEvent::DoneWithDuration {
            context: crate::tui::app::event::UiTurnContext {
                chat_id: ChatId::new("chat-test"),
                turn_id: ChatTurnId::new("turn-test"),
            },
            duration: std::time::Duration::from_secs(1),
        },
        &ui_tx,
        &spawn_refs,
    );

    assert!(
        !app.chat.is_processing,
        "API 错误 turn 收口后 is_processing 必须为 false（下一条输入不进 queue）"
    );
}

/// 收集 System notice timeline 文本（`append_system_notice` 写入 System 块）。
fn system_notice_texts(app: &App) -> Vec<&str> {
    app.model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::System { text, .. } => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn format_reflection_history_accepts_empty_records() {
    assert_eq!(format_reflection_history(&[]), "Reflection history (0):");
}

// ── #1272 debug-safe logging tests ───────────────────────────────────

/// UserMessagesAdopted handler 的 debug log 只记录 text_len，不记录正文。
#[test]
fn user_messages_adopted_handler_logs_text_length_not_preview() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    let input_id = "debug-input".to_string();
    app.enqueue_submission_echo(
        input_id.clone(),
        "some long text that should not appear in logs",
    );

    let items = vec![TuiChatMessage {
        role: "user".to_string(),
        content: vec![TuiContentBlock::text(
            "some long text that should not appear in logs",
        )],
        input_id: Some(input_id.clone()),
        source: TuiMessageSource::User,
        stop_hook: None,
    }];
    app.update_ui(
        UiEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        },
        &ui_tx,
        &spawn_refs,
    );

    // 验证：占位被清除、回显成功（功能不受影响）
    assert!(app.model.conversation.queued_submissions.is_empty());
    // 回显文本正确
    let echoes: Vec<&str> = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|b| {
            if let crate::tui::model::output_timeline::OutputTimelineItem::UserMessage {
                text,
                ..
            } = b
            {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(echoes.iter().any(|t| t.contains("some long text")));
}

#[test]
fn format_reflection_history_renders_optional_metadata_as_absent() {
    let record = sdk::ReflectionHistoryView {
        id: "safe-id".to_string(),
        timestamp: 1,
        trigger: sdk::ReflectionTriggerView::Manual,
        status: sdk::ReflectionStatusView::Running,
        deviations: 0,
        suggestions: 0,
        outdated: 0,
        apply_status: sdk::ReflectionApplyStatusView::NotApplied,
        error_category: None,
        token_usage: None,
        duration_ms: 0,
    };

    let rendered = format_reflection_history(&[record]);
    assert!(rendered.contains("error=none"));
    assert!(rendered.contains("tokens(in/out)=n/a"));
}
