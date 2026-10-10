use super::*;
use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiHookNotice, TuiMessageSource, TuiSkillRequestMetadata,
};
use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::interaction::UiQueuedInputId;
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::SkillsUpdated {
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
        }]),
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::SessionMessageStateChanged {
            message_count: 2,
            revision: 1,
        }]),
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::SessionMessageStateChanged {
            message_count: 2,
            revision: 1,
        }]),
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::SessionMessageStateChanged {
            message_count: 1,
            revision: 1,
        }]),
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

/// TaskData 3: UserMessagesAdopted 按 id 清占位 + 顺序回显
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        }]),
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        }]),
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::UserMessagesAdopted {
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
        }]),
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

/// #749 / #1919：ApiError 退化为纯展示 —— 经 mapping 追加恰好一次 Error notice，
/// NOT 再经 update 侧写 System；且不清 processing（收口交给 Done）。
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::ApiError {
            messages: vec![],
            error: error.clone(),
        }]),
        &ui_tx,
        &spawn_refs,
    );

    // 展示只走 AppendError：Error 恰好一次，System 同文为 0（防双写）
    let error_hits = error_notice_texts(&app)
        .iter()
        .filter(|t| t.contains("stream interrupted after partial output"))
        .count();
    assert_eq!(error_hits, 1, "ApiError 应追加恰好一次 Error notice");
    let system_hits = system_notice_texts(&app)
        .iter()
        .filter(|t| t.contains("stream interrupted after partial output"))
        .count();
    assert_eq!(system_hits, 0, "ApiError 不得再侧写 System notice");

    // ApiError 本身不清 processing —— 收口交给 DoneWithDuration
    assert!(
        app.chat.is_processing,
        "ApiError 不应自行清 processing，收口交给 Done"
    );
}

/// #1919：SessionResumeFailed 只展示一行带前缀的 Error，不得 System+Error 双写。
#[test]
fn test_session_resume_failed_appends_single_prefixed_error() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    let message = "no such session".to_string();
    app.update(
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::SessionResumeFailed {
            kind: crate::tui::adapter::tui_runtime_event::TuiSessionResumeFailureKind::NotFound,
            id: "sess-missing".to_string(),
            message: message.clone(),
        }]),
        &ui_tx,
        &spawn_refs,
    );

    let expected = format!("⚠️ 会话恢复失败（不存在）: {message}");
    let error_hits = error_notice_texts(&app)
        .iter()
        .filter(|t| **t == expected)
        .count();
    assert_eq!(
        error_hits, 1,
        "SessionResumeFailed 应追加恰好一次带前缀 Error"
    );
    let system_hits = system_notice_texts(&app)
        .iter()
        .filter(|t| t.contains(&message))
        .count();
    assert_eq!(system_hits, 0, "SessionResumeFailed 不得侧写 System notice");
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

    // Runtime 权威错误路径：先 ApiError，随后由 Done 收口 processing。
    app.update(
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::ApiError {
            messages: vec![],
            error: "stream error: boom".to_string(),
        }]),
        &ui_tx,
        &spawn_refs,
    );
    assert!(app.chat.is_processing);
    app.update(
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::Done {
            context: crate::tui::adapter::tui_runtime_event::TuiRunContext {
                chat_id: "chat-test".into(),
                run_id: "turn-test".into(),
            },
            duration_ms: Some(1_000),
        }]),
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

/// 收集 Error notice timeline 文本（`AppendError` 写入 Error 块）。
fn error_notice_texts(app: &App) -> Vec<&str> {
    app.model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::Error { text, .. } => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn format_reflection_history_accepts_empty_records() {
    assert_eq!(
        crate::tui::app::update::format_reflection_history(&[]),
        "Reflection history (0):"
    );
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
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::UserMessagesAdopted {
            items,
            queued: vec![],
        }]),
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
    let record = crate::tui::adapter::tui_runtime_event::TuiReflectionRecord {
        id: "safe-id".to_string(),
        timestamp: 1,
        trigger: crate::tui::adapter::tui_runtime_event::TuiReflectionTrigger::Manual,
        status: crate::tui::adapter::tui_runtime_event::TuiReflectionStatus::Running,
        deviations: 0,
        suggestions: 0,
        outdated: 0,
        apply_status: crate::tui::adapter::tui_runtime_event::TuiReflectionApplyStatus::NotApplied,
        error_category: None,
        token_usage: None,
        duration_ms: 0,
        deviation_texts: vec![],
        suggested_memories: vec![],
    };

    let rendered = crate::tui::app::update::format_reflection_history(&[record]);
    assert!(rendered.contains("… Running"), "状态必须符号化: {rendered}");
    assert!(
        !rendered.contains("tok"),
        "无 token 数据时不得显示 tokens 段: {rendered}"
    );
    assert!(
        !rendered.contains("Deviations"),
        "零计数时不得显示 Deviations 组: {rendered}"
    );
}

/// 卡片式渲染：本地时间、状态符号、apply、耗时秒化、tokens、偏差与建议内容全展开。
#[test]
fn format_reflection_history_renders_card_with_content() {
    use crate::tui::adapter::tui_runtime_event::*;
    let record = TuiReflectionRecord {
        id: "r-1".to_string(),
        timestamp: 1_790_878_426,
        trigger: TuiReflectionTrigger::Manual,
        status: TuiReflectionStatus::Succeeded,
        deviations: 1,
        suggestions: 1,
        outdated: 0,
        apply_status: TuiReflectionApplyStatus::Applied,
        error_category: None,
        token_usage: Some((16100, 6312)),
        duration_ms: 84_896,
        deviation_texts: vec!["first deviation\ntwo-line".to_string()],
        suggested_memories: vec![TuiMemorySuggestion {
            layer: TuiMemoryLayer::Project,
            category: TuiMemoryCategory::Decision,
            content: "P5 正在将统一 Runtime Loop 的 fat RunLoopPort 拆分为窄能力边界".to_string(),
            tags: vec![],
            reason: "why".to_string(),
        }],
    };

    let rendered = crate::tui::app::update::format_reflection_history(&[record]);

    let expected_local = chrono::DateTime::from_timestamp(1_790_878_426, 0)
        .expect("valid timestamp")
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
    assert!(
        rendered.contains(&expected_local),
        "必须显示本地时间 {expected_local}: {rendered}"
    );
    assert!(rendered.contains("Manual"));
    assert!(rendered.contains("✓ Succeeded"), "状态符号化: {rendered}");
    assert!(rendered.contains("Applied"), "apply 状态: {rendered}");
    assert!(rendered.contains("84.9s"), "耗时秒化: {rendered}");
    assert!(
        rendered.contains("16100→6312 tok"),
        "tokens 紧凑显示: {rendered}"
    );
    assert!(rendered.contains("Deviations (1):"));
    assert!(
        rendered.contains("first deviation two-line"),
        "多行内容必须折叠为单行: {rendered}"
    );
    assert!(rendered.contains("Suggestions (1):"));
    assert!(rendered.contains("[project/decision]"));
    assert!(rendered.contains("P5 正在将统一 Runtime Loop"));
}

/// 超长内容单行截断（100 字符 + …），不破坏卡片排版。
#[test]
fn format_reflection_history_truncates_long_content() {
    use crate::tui::adapter::tui_runtime_event::*;
    let long = "长".repeat(200);
    let record = TuiReflectionRecord {
        id: "r-2".to_string(),
        timestamp: 1,
        trigger: TuiReflectionTrigger::Interval,
        status: TuiReflectionStatus::Succeeded,
        deviations: 1,
        suggestions: 0,
        outdated: 0,
        apply_status: TuiReflectionApplyStatus::NotApplied,
        error_category: None,
        token_usage: None,
        duration_ms: 500,
        deviation_texts: vec![long.clone()],
        suggested_memories: vec![],
    };

    let rendered = crate::tui::app::update::format_reflection_history(&[record]);
    let truncated = format!("{}…", "长".repeat(100));
    assert!(
        rendered.contains(&truncated),
        "必须截断为 100 字符: {rendered}"
    );
    assert!(
        !rendered.contains(&"长".repeat(150)),
        "不得保留完整长文: {rendered}"
    );
    assert!(rendered.contains("500ms"), "短耗时保持毫秒: {rendered}");
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
            TuiRuntimeEvent::AssistantTextDelta {
                context: context.clone(),
                delta: "first ".to_string(),
            },
            TuiRuntimeEvent::AssistantTextDelta {
                context: context.clone(),
                delta: "second".to_string(),
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

/// 图片路径不可用时（终端粘贴的转义路径、飞书贴纸缓存被清理等），
/// 回灌事件必须把原始粘贴文本插回输入区，绝不吞掉用户内容。
#[test]
fn paste_fallback_inserts_original_text_when_idle() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let pasted = "/Users/me/Library/Application\\ Support/stickers/a.gif".to_string();

    app.update_ui(
        UiEvent::PasteFallbackToText {
            text: pasted.clone(),
        },
        &ui_tx,
        &make_spawn_refs(),
    );

    assert_eq!(app.model.input.document.buffer, pasted);
}

#[test]
fn paste_fallback_inserts_original_text_while_processing() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    app.chat.start_processing();

    app.update_ui(
        UiEvent::PasteFallbackToText {
            text: "https://example.com/assets/diagram.png".to_string(),
        },
        &ui_tx,
        &make_spawn_refs(),
    );

    assert_eq!(
        app.model.input.document.buffer,
        "https://example.com/assets/diagram.png"
    );
}

/// #1816 跨层场景：runtime 快照事件 → intent → model → 队列行渲染。
///
/// 覆盖 busy 期间提交控制类斜杠命令的回显链路：命令进入排队行，
/// runtime 执行后发空快照，队列行随之消失。
#[test]
fn test_control_command_queue_event_renders_and_clears_queue_lines() {
    let mut app = test_app();
    let spawn_refs = make_spawn_refs();

    let (ui_tx, _ui_rx) = mpsc::channel(4);
    app.update(
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::ControlCommandsQueued {
            queued: vec![
                (
                    UiQueuedInputId::from("01920000-0000-7000-8000-000000000001"),
                    "/compact".to_string(),
                ),
                (
                    UiQueuedInputId::from("01920000-0000-7000-8000-000000000002"),
                    "/model anthropic/claude".to_string(),
                ),
            ],
        }]),
        &ui_tx,
        &spawn_refs,
    );

    assert_eq!(
        app.live_status_view_model().queued_lines,
        vec!["> /compact", "> /model anthropic/claude"],
        "排队命令必须出现在状态行，否则用户看不到命令已被接收"
    );

    app.update(
        TuiMsg::RuntimeBatch(vec![TuiRuntimeEvent::ControlCommandsQueued {
            queued: vec![],
        }]),
        &ui_tx,
        &spawn_refs,
    );

    assert!(
        app.live_status_view_model().queued_lines.is_empty(),
        "命令执行后 runtime 发空快照，队列行必须清空"
    );
}

/// #1816：消息与命令混排时按入队序号合并成提交顺序。
#[test]
fn test_queued_messages_and_commands_merge_in_arrival_order() {
    let mut app = test_app();
    app.enqueue_submission_echo("01920000-0000-7000-8000-000000000001", "先到的消息");
    let (ui_tx, _ui_rx) = mpsc::channel(4);
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

    assert_eq!(
        app.live_status_view_model().queued_lines,
        vec!["> 先到的消息", "> /compact"],
        "两类占位必须按入队序号合并，否则用户看到的顺序与提交顺序不符"
    );
}
