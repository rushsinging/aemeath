use super::*;
use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiHookNotice, TuiMessageSource, TuiSkillRequestMetadata,
};
use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::ids::{ChatId, ChatRunId};
use crate::tui::update::msg::TuiMsg;
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

#[test]
fn display_history_window_failure_clears_inflight_request_for_retry() {
    let mut app = test_app();
    let request = sdk::DisplayHistoryWindowRequest {
        session_id: "retry-session".to_string(),
        generation_revision: 9,
        member_names: vec!["steps/0009.json".to_string()],
    };
    app.output_view.loading_history_window = Some((
        request.session_id.clone(),
        request.generation_revision,
        request.member_names.clone(),
    ));
    let (ui_tx, _ui_rx) = mpsc::channel(1);

    app.update_ui(
        UiEvent::DisplayHistoryWindowLoadFailed {
            request,
            message: "读取失败".to_string(),
        },
        &ui_tx,
        &make_spawn_refs(),
    );

    assert!(app.output_view.loading_history_window.is_none());
}

#[test]
fn skills_updated_atomically_rebuilds_qualified_route_and_completion_catalog() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();
    app.model.input.document.buffer = "/super".to_string();
    app.model.input.document.cursor = "/super".len();

    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::SkillsUpdated {
            revision: "r1".to_string(),
            skills: vec![crate::tui::adapter::tui_runtime_event::TuiSkillView {
                name: "superpowers:brainstorming".to_string(),
                aliases: vec!["brainstorming".to_string()],
                slash_command: Some("superpowers:brainstorming".to_string()),
                slash_aliases: Vec::new(),
                description: "Explore requirements".to_string(),
                argument_hint: None,
            }],
            slash_routes: vec![crate::tui::adapter::tui_runtime_event::TuiSkillSlashRoute {
                skill: "superpowers:brainstorming".to_string(),
                slash_command: "superpowers:brainstorming".to_string(),
                aliases: Vec::new(),
                argument_hint: None,
            }],
        }),
        &ui_tx,
        &spawn_refs,
    );

    assert_eq!(app.skill_completion_catalog.revision, "r1");
    assert!(matches!(
        app.skill_completion_catalog.resolve("/superpowers:brainstorming idea"),
        Some(command)
            if command.skill == "superpowers:brainstorming"
                && command.arguments.as_slice() == ["idea"]
    ));
    assert!(matches!(
        app.command_router
            .as_deref()
            .expect("router")
            .resolve(sdk::SlashInput::new("/superpowers:brainstorming idea")),
        Err(sdk::CommandParseError::UnknownCommand { .. })
    ));
    assert_eq!(app.model.input.completion.items.len(), 1);
    assert_eq!(
        app.model.input.completion.items[0].replacement,
        "/superpowers:brainstorming"
    );
}

/// 消息状态投影只更新 session metadata，不产生 UserMessage 回显块，
/// 也不清除占位（回显与占位清理由 UserMessagesAdopted 负责）。
#[test]
fn test_update_message_state_only_updates_metadata_without_echo() {
    let mut app = test_app();
    let echo_id = "echo-1".to_string();
    app.enqueue_submission_echo(echo_id, "[Copied Text 1]");
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = SpawnContextRefs { agent_client: None };

    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::SessionMessageStateChanged {
            message_count: 2,
            revision: 1,
        }),
        &ui_tx,
        &spawn_refs,
    );

    assert_eq!(app.model.session.message_count, 2);
    assert!(app.model.conversation.timeline.items().iter().all(|item| {
        !matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. } if text == "a\nb\nc")
    }));
    assert_eq!(
        app.model.conversation.queued_submissions.len(),
        1,
        "消息状态投影不应清占位"
    );
}

#[test]
fn test_update_ui_post_tool_sync_does_not_echo_system_generated_user_message() {
    let mut app = test_app();
    let reminder = "<system-reminder>\nStop hook blocked stopping.\n</system-reminder>";
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = SpawnContextRefs { agent_client: None };

    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::SessionMessageStateChanged {
            message_count: 2,
            revision: 1,
        }),
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
/// - handler 后 SessionMessageStateChanged 不再镜像 chat.messages（字段已删除）
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

    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::SessionMessageStateChanged {
            message_count: 1,
            revision: 1,
        }),
        &ui_tx,
        &spawn_refs,
    );

    // SessionMessageStateChanged 不再镜像 chat.messages（字段已删除）
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
            hook_notice: None,
            skill_request: None,
        },
        TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text("yo")],
            input_id: Some(id_b.clone()),
            source: TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
        },
    ];
    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        }),
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
        hook_notice: None,
        skill_request: None,
    }];

    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        }),
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

#[test]
fn adopted_typed_skill_and_hook_notice_keep_distinct_semantics_without_user_echoes() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();
    let skill_id = "skill-input".to_string();
    let hook_id = "hook-input".to_string();
    app.enqueue_submission_echo(skill_id.clone(), "/superpowers:brainstorming feature scope");
    app.enqueue_submission_echo(hook_id.clone(), "hook feedback");

    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::UserMessagesAdopted {
            items: vec![
                TuiChatMessage {
                    role: "user".to_string(),
                    content: vec![TuiContentBlock::text("LLM skill prompt")],
                    input_id: Some(skill_id),
                    source: TuiMessageSource::SkillRequest,
                    hook_notice: None,
                    skill_request: Some(TuiSkillRequestMetadata {
                        skill: "superpowers:brainstorming".to_string(),
                        arguments: "feature scope".to_string(),
                        raw_input: "/superpowers:brainstorming feature scope".to_string(),
                    }),
                },
                TuiChatMessage {
                    role: "user".to_string(),
                    content: vec![TuiContentBlock::text("LLM hook prompt")],
                    input_id: Some(hook_id),
                    source: TuiMessageSource::Hook,
                    hook_notice: Some(TuiHookNotice {
                        point: "Stop".to_string(),
                        kind: crate::tui::adapter::runtime_view::TuiHookNoticeKind::Blocked,
                        summary: "blocked".to_string(),
                        command: "check.sh".to_string(),
                        exit_code: Some(2),
                        reason: "guard failed".to_string(),
                        stdout_preview: "details".to_string(),
                        stderr_preview: "blocked".to_string(),
                        stdout_truncated: false,
                        stderr_truncated: false,
                        output_file: None,
                    }),
                    skill_request: None,
                },
            ],
            queued: Vec::new(),
        }),
        &ui_tx,
        &spawn_refs,
    );

    assert!(app.model.conversation.queued_submissions.is_empty());
    let user_echo_texts: Vec<_> = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::UserMessage {
                text, ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        user_echo_texts,
        vec!["/superpowers:brainstorming feature scope"]
    );
    let notices = system_notice_texts(&app);
    assert!(!notices.iter().any(|text| text.contains("raw_input")));
    assert!(!notices.iter().any(|text| text.contains("<skill-request>")));
    assert!(!notices.iter().any(|text| text.contains("guard failed")));
    assert!(app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .any(|item| matches!(
            item,
            crate::tui::model::output_timeline::OutputTimelineItem::HookNotice { title, text, .. }
                if title == "Stop hook blocked" && text.contains("guard failed")
        )));
    assert!(!notices.iter().any(|text| text.contains("LLM skill prompt")));
    assert!(!notices.iter().any(|text| text.contains("LLM hook prompt")));
}

/// Compact 完成只清理 compact detail；Run 生命周期继续由 typed status 所有。
#[test]
fn test_messages_sync_clears_compact_runtime_state() {
    use crate::tui::model::conversation::intent::SetCompactProgress;

    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    app.model.conversation.apply(SetCompactProgress {
        stage: "finalizing".into(),
        current: Some(8),
        total: Some(10),
    });
    assert!(
        app.model.conversation.runtime.compact_progress.is_some(),
        "precondition: compact_progress 已设置"
    );

    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::CompactFinished {
            messages: vec![],
            notice: "✓ 上下文压缩完成".to_string(),
        }),
        &ui_tx,
        &spawn_refs,
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
    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::ApiError {
            messages: vec![],
            error: error.clone(),
        }),
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
                run_id: ChatRunId::new("turn-test"),
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
        hook_notice: None,
        skill_request: None,
    }];
    app.update(
        TuiMsg::Runtime(TuiRuntimeEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        }),
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

#[test]
fn runtime_batch_applies_all_events_before_the_next_render() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();
    let context = crate::tui::adapter::tui_runtime_event::TuiRunContext {
        chat_id: "batch-chat".to_string(),
        run_id: "batch-turn".to_string(),
    };

    let result = app.update(
        TuiMsg::RuntimeBatch(vec![
            TuiRuntimeEvent::Text {
                context: context.clone(),
                text: "first ".to_string(),
            },
            TuiRuntimeEvent::Text {
                context: context.clone(),
                text: "second".to_string(),
            },
            TuiRuntimeEvent::BlockComplete {
                context,
                text: "first second".to_string(),
            },
        ]),
        &ui_tx,
        &spawn_refs,
    );

    let assistant = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .find_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::AssistantText {
                text, ..
            } => Some(text.as_str()),
            _ => None,
        });
    assert_eq!(assistant, Some("first second"));
    assert!(app.view_state.dirty.output);
    assert_eq!(
        result
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::RequestRender))
            .count(),
        1
    );
}
