use super::App;
use crate::tui::effect::effect::Effect;

/// App::new 注入真实 builtin CommandRouter（`wire_commands()`），
/// 因此这里的分发测试直接对真实 specs 表生效（含退役命令回归）。
fn app_with_builtin_router() -> App {
    App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    )
}

fn sent_chat_event(result: &crate::tui::app::update::UpdateResult) -> Option<&sdk::ChatInputEvent> {
    result.effects.iter().find_map(|effect| match effect {
        crate::tui::effect::effect::Effect::SendChatInputEvent { event } => Some(event),
        _ => None,
    })
}

fn system_texts(app: &App) -> Vec<&str> {
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

fn error_texts(app: &App) -> Vec<&str> {
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

fn apply_runtime_event(
    app: &mut App,
    event: crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent,
) {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let spawn_refs =
        crate::tui::effect::session::processing::SpawnContextRefs { agent_client: None };
    app.update(
        crate::tui::update::msg::TuiMsg::RuntimeBatch(vec![event]),
        &tx,
        &spawn_refs,
    );
}

#[test]
fn compact_slash_command_returns_send_event_effect() {
    let mut app = app_with_builtin_router();

    let result = app.handle_slash_command("/compact");

    assert!(
        matches!(sent_chat_event(&result), Some(sdk::ChatInputEvent::Compact)),
        "/compact 应产出 SendChatInputEvent{{Compact}} effect，实际: {:?}",
        result.effects
    );
}

#[test]
fn skill_request_routes_to_send_event_effect() {
    let mut app = App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    app.set_skill_snapshot(sdk::SkillsUpdatedEvent {
        revision: "startup-r1".to_string(),
        skills: vec![sdk::SkillView {
            name: "archify".to_string(),
            aliases: Vec::new(),
            slash_command: Some("archify".to_string()),
            slash_aliases: Vec::new(),
            description: "Create diagrams".to_string(),
            argument_hint: None,
        }],
        slash_routes: vec![sdk::SkillSlashRouteView {
            skill: "archify".to_string(),
            slash_command: "archify".to_string(),
            aliases: Vec::new(),
            argument_hint: None,
        }],
    });

    let result = app.handle_slash_command("/archify runtime flow");

    match sent_chat_event(&result) {
        Some(sdk::ChatInputEvent::SkillRequest(request)) => {
            assert_eq!(request.skill, "archify");
            assert_eq!(request.arguments, "runtime flow");
            assert_eq!(request.raw_input, "/archify runtime flow");
        }
        other => panic!(
            "/archify 应产出 SkillRequest 事件 effect，实际: {:?}（effects: {:?}）",
            other, result.effects
        ),
    }
}

/// 退役回归：/status、/images、/clear-images、/save、/paste、/rewind
/// 已从 builtin 表删除，router 返回 UnknownCommand，分发只给 error notice、零 effect。
#[test]
fn retired_slash_commands_are_rejected_with_error_notice() {
    for input in [
        "/status",
        "/images",
        "/clear-images",
        "/save",
        "/paste",
        "/rewind 3",
    ] {
        let mut app = app_with_builtin_router();
        let result = app.handle_slash_command(input);

        assert!(
            result.effects.is_empty(),
            "退役命令 {input} 不应产出 effect，实际: {:?}",
            result.effects
        );
        let errors = error_texts(&app);
        assert!(
            errors.iter().any(|text| text.contains("未知命令")),
            "退役命令 {input} 应显示 router 错误，实际 notices: {errors:?}"
        );
    }
}

#[test]
fn reflect_slash_command_returns_query_effect() {
    let mut app = app_with_builtin_router();

    let result = app.handle_slash_command("/reflect 3");

    assert!(
        result.effects.iter().any(|effect| matches!(
            effect,
            crate::tui::effect::effect::Effect::QueryReflectionHistory { limit: 3 }
        )),
        "/reflect 3 应产出 QueryReflectionHistory{{limit:3}} effect，实际: {:?}",
        result.effects
    );
}

#[test]
fn memory_remind_slash_command_returns_fetch_effect() {
    let mut app = app_with_builtin_router();

    let result = app.handle_slash_command("/memory remind");

    assert!(
        result
            .effects
            .iter()
            .any(|effect| matches!(effect, crate::tui::effect::effect::Effect::FetchMemoryList)),
        "/memory remind 应产出 FetchMemoryList effect，实际: {:?}",
        result.effects
    );
}

#[test]
fn update_slash_command_returns_self_update_effect() {
    let mut app = app_with_builtin_router();

    let result = app.handle_slash_command("/update");

    assert!(
        result
            .effects
            .iter()
            .any(|effect| matches!(effect, crate::tui::effect::effect::Effect::RunSelfUpdate)),
        "/update 应产出 RunSelfUpdate effect，实际: {:?}",
        result.effects
    );
}

#[test]
fn test_clear_command_clears_task_store_and_task_window() {
    let mut app = app_with_builtin_router();
    app.model
        .conversation
        .apply(crate::tui::model::conversation::intent::UpdateTaskLines(
            vec!["━━ Tasks: 1/1 ━━".to_string(), "□ #1 existing".to_string()],
        ));
    app.refresh_live_status_from_model();

    let result = app.handle_slash_command("/clear");
    app.refresh_live_status_from_model();

    // loop 未运行（无 input_event_tx）：本地同步 reset，零 effect。
    assert!(result.effects.is_empty());
    assert!(app.model.conversation.runtime.task_status.lines.is_empty());
    assert!(app.live_status_view_model().task_lines.is_empty());
}

#[test]
fn reflection_history_displays_safe_metadata_without_body() {
    let mut app = App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    let event = crate::tui::adapter::event_mapping::sdk_event_to_tui_event(
        sdk::ChatEvent::ReflectionHistory {
            records: vec![sdk::ReflectionHistoryView {
                id: "reflection-secret-body-must-not-appear".to_string(),
                timestamp: 1_700_000_000,
                trigger: sdk::ReflectionTriggerView::Manual,
                status: sdk::ReflectionStatusView::Failed,
                deviations: 2,
                suggestions: 3,
                outdated: 1,
                apply_status: sdk::ReflectionApplyStatusView::PartiallyApplied,
                error_category: Some(sdk::ReflectionErrorCategoryView::Parse),
                token_usage: Some(sdk::ReflectionTokenUsageView {
                    input_tokens: 11,
                    output_tokens: 7,
                }),
                duration_ms: 432,
            }],
        },
    );
    let crate::tui::adapter::event_mapping::SdkEventMapping::Runtime(event) = event else {
        panic!("reflection history must map to one runtime event");
    };
    apply_runtime_event(&mut app, event);

    let rendered = system_texts(&app).join("\n");
    assert!(rendered.contains("timestamp=1700000000"));
    assert!(rendered.contains("trigger=Manual"));
    assert!(rendered.contains("status=Failed"));
    assert!(rendered.contains("2/3/1"));
    assert!(rendered.contains("apply=PartiallyApplied"));
    assert!(rendered.contains("error=Parse"));
    assert!(rendered.contains("tokens(in/out)=11/7"));
    assert!(rendered.contains("duration=432ms"));
    assert!(!rendered.contains("reflection-secret-body-must-not-appear"));
}

/// #1092 缺口回归：`/memory remind` 的结果（ReminderList 回传）必须渲染为
/// 系统 notice——active/done 状态与 content 可见，空列表有明确反馈。
#[test]
fn memory_remind_renders_returned_reminder_list_as_system_notice() {
    let mut app = app_with_builtin_router();

    let result = app.handle_slash_command("/memory remind");
    assert!(
        result
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::FetchMemoryList)),
        "/memory remind 应产出 FetchMemoryList effect，实际: {:?}",
        result.effects
    );

    let mapping =
        crate::tui::adapter::event_mapping::sdk_event_to_tui_event(sdk::ChatEvent::ReminderList {
            reminders: vec![
                sdk::ReminderView {
                    id: "reminder-1".to_owned(),
                    content: "drink water".to_owned(),
                    done: false,
                    created_at: 1_700_000_000,
                },
                sdk::ReminderView {
                    id: "reminder-2".to_owned(),
                    content: "ship release".to_owned(),
                    done: true,
                    created_at: 1_700_000_100,
                },
            ],
        });
    let crate::tui::adapter::event_mapping::SdkEventMapping::Runtime(event) = mapping else {
        panic!("ReminderList must map to one runtime event");
    };
    apply_runtime_event(&mut app, event);

    let rendered = system_texts(&app).join("\n");
    assert!(
        rendered.contains("drink water"),
        "active reminder 内容应可见"
    );
    assert!(
        rendered.contains("ship release"),
        "done reminder 内容应可见"
    );
    assert!(rendered.contains("Reminders"), "应有列表标题");

    // 空列表反馈。
    let empty_mapping =
        crate::tui::adapter::event_mapping::sdk_event_to_tui_event(sdk::ChatEvent::ReminderList {
            reminders: Vec::new(),
        });
    let crate::tui::adapter::event_mapping::SdkEventMapping::Runtime(empty_event) = empty_mapping
    else {
        panic!("empty ReminderList must map to one runtime event");
    };
    apply_runtime_event(&mut app, empty_event);
    let rendered_after_empty = system_texts(&app).join("\n");
    assert!(
        rendered_after_empty.contains("No reminders"),
        "空列表应有明确反馈，实际: {rendered_after_empty}"
    );
}

/// A 类 slash 命令（纯本地状态写入）表驱动回归：本地 notice / 退出标志 +
/// 零 effect、零输入事件——#947 纯化后这些命令不得触发任何 I/O。
#[test]
fn local_slash_commands_render_notices_without_side_effects() {
    let cases: &[(&str, &str)] = &[
        ("/help", "Commands:"),
        ("/usage", "API calls:"),
        ("/cost", "API calls:"),
        ("/context", "Messages:"),
        ("/config", "Model:"),
        ("/stats", "API calls:"),
        ("/version", "aemeath v"),
        ("/doctor", "Doctor"),
    ];
    for (input, expected_fragment) in cases {
        let mut app = app_with_builtin_router();

        let result = app.handle_slash_command(input);

        assert!(
            result.effects.is_empty(),
            "A 类命令 {input} 不应产出 effect，实际: {:?}",
            result.effects
        );
        let rendered = system_texts(&app).join("\n");
        assert!(
            rendered.contains(expected_fragment),
            "{input} 应渲染包含 {expected_fragment:?} 的 notice，实际: {rendered}"
        );
    }

    // /exit：同步设置退出标志，零 effect。
    let mut app = app_with_builtin_router();
    let result = app.handle_slash_command("/exit");
    assert!(result.effects.is_empty());
    assert!(app.layout.should_exit, "/exit 应设置退出标志");
}

// === #740：/model 对话框与 /resume 补全的事件流数据源 ===

fn model_summary(provider: &str, id: &str, name: &str) -> sdk::ModelSummary {
    sdk::ModelSummary {
        provider: provider.to_string(),
        id: id.to_string(),
        name: name.to_string(),
        context_window: 200_000,
        max_tokens: 8_000,
    }
}

fn session_summary(id: &str, summary: &str) -> sdk::SessionSummary {
    sdk::SessionSummary {
        id: id.to_string(),
        title: None,
        project: None,
        model: None,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-02T00:00:00Z".to_string(),
        message_count: 3,
        preview: None,
        summary: summary.to_string(),
    }
}

/// 走真实 sdk → TUI 映射后再 apply，覆盖 adapter 映射与 update 消费两层。
fn apply_sdk_event(app: &mut App, event: sdk::ChatEvent) {
    let mapping = crate::tui::adapter::event_mapping::sdk_event_to_tui_event(event);
    let crate::tui::adapter::event_mapping::SdkEventMapping::Runtime(runtime_event) = mapping
    else {
        panic!("ModelList/SessionList 必须映射为单个 runtime 事件");
    };
    apply_runtime_event(app, runtime_event);
}

/// #740：缓存未回填时打开 /model 必须发 `ListModels` 请求并挂起等待，
/// NEVER 直接显示 "No models configured"。
#[test]
fn model_dialog_without_cached_models_requests_list_and_waits() {
    let mut app = app_with_builtin_router();

    let result = app.handle_slash_command("/model");

    assert!(
        matches!(
            sent_chat_event(&result),
            Some(sdk::ChatInputEvent::ListModels)
        ),
        "/model 缓存未回填时应发 ListModels 请求，实际: {:?}",
        result.effects
    );
    assert!(
        app.session.model_selection_pending,
        "缓存未回填时应挂起等待事件回填"
    );
    assert!(
        !app.layout.has_active_dialog(),
        "缓存未回填时不得直接打开 dialog"
    );
    let rendered = system_texts(&app).join("\n");
    assert!(
        rendered.contains("Loading model list"),
        "挂起时应提示加载中，实际: {rendered}"
    );
}

/// #740：`ModelList` 事件回填缓存后，挂起的 /model 对话框必须自动打开。
#[test]
fn model_list_event_fills_cache_and_opens_pending_dialog() {
    let mut app = app_with_builtin_router();
    let _ = app.handle_slash_command("/model");

    apply_sdk_event(
        &mut app,
        sdk::ChatEvent::ModelList {
            models: vec![model_summary("anthropic", "claude-3-id", "Claude 3")],
        },
    );

    let cached = app
        .session
        .cached_models
        .as_ref()
        .expect("ModelList 事件应回填模型缓存");
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].provider, "anthropic");
    assert!(
        !app.session.model_selection_pending,
        "事件回填后应清除挂起标志"
    );
    assert!(
        app.layout.has_active_dialog(),
        "挂起的 dialog 应在事件回填后打开"
    );
    assert_eq!(
        app.layout.dialog_model_keys,
        vec!["anthropic/Claude 3".to_string()],
        "dialog 选择 key 应为 provider/name"
    );
}

/// #740：runtime 确认模型列表为空时，提示必须指向真实配置路径
/// （`~/.agents/aemeath.json` / `.agents/aemeath.json`），
/// NEVER 再出现过期的 `~/.aemeath/config.json`。
#[test]
fn loaded_empty_model_list_shows_real_config_path() {
    let mut app = app_with_builtin_router();
    apply_sdk_event(&mut app, sdk::ChatEvent::ModelList { models: vec![] });

    let result = app.handle_slash_command("/model");

    assert!(
        matches!(
            sent_chat_event(&result),
            Some(sdk::ChatInputEvent::ListModels)
        ),
        "空列表场景仍应发 ListModels 刷新，实际: {:?}",
        result.effects
    );
    assert!(!app.layout.has_active_dialog());
    assert!(!app.session.model_selection_pending);
    let rendered = system_texts(&app).join("\n");
    assert!(
        rendered.contains("~/.agents/aemeath.json") && rendered.contains(".agents/aemeath.json"),
        "空态提示必须指向真实全局/项目配置路径，实际: {rendered}"
    );
    assert!(
        !rendered.contains("~/.aemeath/config.json"),
        "不得再出现过期配置路径，实际: {rendered}"
    );
}

/// #740：缓存已回填时 /model 立即打开 dialog（不等待），并附带一次刷新请求。
#[test]
fn cached_models_open_dialog_immediately_with_refresh() {
    let mut app = app_with_builtin_router();
    apply_sdk_event(
        &mut app,
        sdk::ChatEvent::ModelList {
            models: vec![
                model_summary("anthropic", "claude-3-id", "Claude 3"),
                model_summary("openai", "gpt-5-id", "GPT-5"),
            ],
        },
    );

    let result = app.handle_slash_command("/model");

    assert!(
        app.layout.has_active_dialog(),
        "缓存已回填时应立即打开 dialog"
    );
    assert_eq!(
        app.layout.dialog_model_keys,
        vec!["anthropic/Claude 3".to_string(), "openai/GPT-5".to_string()]
    );
    assert!(
        matches!(
            sent_chat_event(&result),
            Some(sdk::ChatInputEvent::ListModels)
        ),
        "打开 dialog 应同时发 ListModels 刷新，实际: {:?}",
        result.effects
    );
}

/// #740：`SessionList` 事件必须回填 /resume 补全数据源，
/// 输入 `/resume ` 时产出历史 session 候选。
#[test]
fn session_list_event_feeds_resume_completion() {
    use crate::tui::model::input::intent::InputIntent;

    let mut app = app_with_builtin_router();
    apply_sdk_event(
        &mut app,
        sdk::ChatEvent::SessionList {
            sessions: vec![
                session_summary("s-100", "first session"),
                session_summary("s-200", "second session"),
            ],
        },
    );

    assert_eq!(
        app.session.cached_sessions,
        vec![
            ("s-100".to_string(), "first session".to_string()),
            ("s-200".to_string(), "second session".to_string())
        ],
        "SessionList 事件应回填 (id, summary) 缓存"
    );

    app.model
        .input
        .apply(InputIntent::ReplaceText("/resume ".to_string()));
    app.update_suggestions();

    let completion = &app.model.input.completion;
    assert!(completion.visible, "/resume 应展示 session 补全候选");
    assert!(
        completion
            .items
            .iter()
            .any(|item| item.label.contains("s-100")),
        "补全候选应包含回填的 session id，实际: {:?}",
        completion.items
    );
}
