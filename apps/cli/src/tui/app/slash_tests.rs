use super::App;

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

/// #1289：/reflect-now 产出 ReflectNow typed 事件；busy 丢弃语义归 Runtime gate。
#[test]
fn reflect_now_slash_command_returns_send_event_effect() {
    let mut app = app_with_builtin_router();

    let result = app.handle_slash_command("/reflect-now");

    assert!(
        matches!(
            sent_chat_event(&result),
            Some(sdk::ChatInputEvent::ReflectNow)
        ),
        "/reflect-now 应产出 SendChatInputEvent{{ReflectNow}} effect，实际: {:?}",
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
                deviation_texts: vec![],
                suggested_memories: vec![],
            }],
        },
    );
    let crate::tui::adapter::event_mapping::SdkEventMapping::Runtime(event) = event else {
        panic!("reflection history must map to one runtime event");
    };
    apply_runtime_event(&mut app, event);

    let rendered = system_texts(&app).join("\n");
    let expected_local = chrono::DateTime::from_timestamp(1_700_000_000, 0)
        .expect("valid timestamp")
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
    assert!(rendered.contains(&expected_local), "本地时间: {rendered}");
    assert!(rendered.contains("Manual"));
    assert!(rendered.contains("✗ Failed (Parse)"));
    assert!(rendered.contains("PartiallyApplied"));
    assert!(rendered.contains("432ms"));
    assert!(rendered.contains("11→7 tok"));
    assert!(rendered.contains("Deviations (2):"));
    assert!(rendered.contains("Suggestions (3):"));
    assert!(rendered.contains("1 outdated"));
    assert!(!rendered.contains("reflection-secret-body-must-not-appear"));
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

fn session_summary(id: &str, summary: &str) -> sdk::SessionItemList {
    sdk::SessionItemList {
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
    // 显示时刷新：缓存标记为空也必须等 fresh 列表到达后再呈现结论。
    assert!(!app.layout.has_active_dialog());
    assert!(
        app.session.model_selection_pending,
        "刷新在途时应挂起等待，不得用旧缓存直接下结论"
    );

    apply_sdk_event(&mut app, sdk::ChatEvent::ModelList { models: vec![] });

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

/// 显示时刷新：缓存已有内容也 MUST 等 fresh 列表到达后才渲染 dialog，
/// 否则用户看到的是配置修改前的旧模型。
#[test]
fn cached_models_wait_for_fresh_list_before_opening_dialog() {
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
        matches!(
            sent_chat_event(&result),
            Some(sdk::ChatInputEvent::ListModels)
        ),
        "打开 dialog 前必须发 ListModels 刷新，实际: {:?}",
        result.effects
    );
    assert!(
        !app.layout.has_active_dialog(),
        "旧缓存不得直接打开 dialog，必须等 fresh 列表"
    );
    assert!(
        app.session.model_selection_pending,
        "刷新在途时应挂起等待 fresh 列表"
    );

    apply_sdk_event(
        &mut app,
        sdk::ChatEvent::ModelList {
            models: vec![
                model_summary("anthropic", "claude-3-id", "Claude 3"),
                model_summary("openai", "gpt-5-id", "GPT-5"),
                model_summary("kimi", "k3-1-pro", "K3.1 Pro"),
            ],
        },
    );

    assert!(
        app.layout.has_active_dialog(),
        "fresh 列表到达后应打开 dialog"
    );
    assert_eq!(
        app.layout.dialog_model_keys,
        vec![
            "anthropic/Claude 3".to_string(),
            "openai/GPT-5".to_string(),
            "kimi/K3.1 Pro".to_string()
        ],
        "dialog 必须渲染 fresh 列表（含配置新增的模型）"
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

/// /resume 按需拉取：首次输入 /resume 触发一次 session 列表请求；
/// 标记置位后不重复请求（避免每次按键都打磁盘扫描）。
#[test]
fn resume_input_fetches_session_list_once() {
    let mut app = app_with_builtin_router();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    app.chat.input_event_tx = Some(event_tx);

    // 常驻 loop 未跑（tx 未连接 runtime）时 push 仍然入队成功，
    // 事件在 runtime attach 后被消费。
    app.model.input.document.buffer = "/resume".to_string();
    app.update_suggestions();
    assert!(
        app.session.session_list_requested,
        "首次输入 /resume 必须置位请求标记"
    );

    app.model.input.document.buffer = "/resume 01a0".to_string();
    app.update_suggestions();
    let mut received = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        received.push(event);
    }
    assert_eq!(
        received.len(),
        1,
        "session 列表请求只应发送一次，实际 {received:?}"
    );
    assert!(
        matches!(
            received.first(),
            Some(sdk::ChatInputEvent::ManageSession { args }) if args == "list"
        ),
        "请求应为 ManageSession list：{received:?}"
    );
}

/// 设置输入框内容并把光标移到末尾（补全触发依赖光标位置）。
fn set_input(app: &mut App, text: &str) {
    app.model.input.document.buffer = text.to_string();
    app.model.input.document.cursor = text.len();
}

/// 显示时刷新：进入 `/model <arg>` 补全时请求一次模型列表（pull，不靠事件推送）；
/// 同一轮补全输入不重复请求，离开补全后再次进入会重新请求。
#[test]
fn model_arg_completion_fetches_model_list_once_per_entry() {
    let mut app = app_with_builtin_router();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    app.chat.input_event_tx = Some(event_tx);

    set_input(&mut app, "/model ");
    app.update_suggestions();
    assert!(
        app.session.model_completion_list_requested,
        "进入 /model 参数补全时必须置位刷新标记"
    );

    set_input(&mut app, "/model kim");
    app.update_suggestions();

    let mut received = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        received.push(event);
    }
    assert_eq!(
        received.len(),
        1,
        "同一轮补全只应请求一次模型列表，实际 {received:?}"
    );
    assert!(
        matches!(received.first(), Some(sdk::ChatInputEvent::ListModels)),
        "请求应为 ListModels：{received:?}"
    );

    // 离开补全 → 标记复位 → 再次进入重新拉取（配置可能已变化）。
    set_input(&mut app, "/think");
    app.update_suggestions();
    assert!(
        !app.session.model_completion_list_requested,
        "离开 /model 参数补全后必须复位刷新标记"
    );

    set_input(&mut app, "/model k");
    app.update_suggestions();
    let mut second_round = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        second_round.push(event);
    }
    assert_eq!(
        second_round.len(),
        1,
        "重新进入补全应再请求一次，实际 {second_round:?}"
    );
}

// ── #2032：/think 档位调节 ──
#[test]
fn think_slash_command_with_level_returns_set_thinking_event() {
    let mut app = app_with_builtin_router();
    let result = app.handle_slash_command("/think high");

    match sent_chat_event(&result) {
        Some(sdk::ChatInputEvent::SetThinking {
            request: sdk::ThinkingRequest::Level(level),
        }) => assert_eq!(*level, sdk::ReasoningLevel::High),
        other => panic!("/think high 应发送 SetThinking Level(high)，实际 {other:?}"),
    }
}

#[test]
fn think_slash_command_with_invalid_level_reports_error_without_event() {
    let mut app = app_with_builtin_router();
    let result = app.handle_slash_command("/think turbo");

    assert!(sent_chat_event(&result).is_none(), "非法档位不得发送事件");
    assert!(
        error_texts(&app)
            .iter()
            .any(|text| text.contains("Unknown reasoning level")),
        "应提示非法档位错误，实际 {:?}",
        error_texts(&app)
    );
}

#[test]
fn think_slash_command_without_args_opens_level_dialog() {
    let mut app = app_with_builtin_router();
    let result = app.handle_slash_command("/think");

    assert!(
        sent_chat_event(&result).is_none(),
        "/think 无参数不应发送事件"
    );
    let dialog = app
        .layout
        .active_dialog()
        .expect("/think 无参数应打开档位对话框");
    let options = dialog.options();
    assert_eq!(options.len(), 7, "档位全集固定 7 档，实际 {options:?}");
    assert_eq!(options[0], "off", "首项为 off（无当前标记）");
}

#[test]
fn think_dialog_annotates_mapping_against_model_capability() {
    use crate::tui::view_model::status::ReasoningLevelView as Level;
    let mut app = app_with_builtin_router();
    // 构造含降档条目的映射表（表由 runtime resolve 权威给出；TUI 对任意
    // 表内容只负责渲染——覆盖离散阶梯空隙降档场景，如 minimal → off）。
    let ladder_pairs = [
        (Level::Off, Level::Off),
        (Level::Minimal, Level::Off),
        (Level::Low, Level::Off),
        (Level::Medium, Level::Medium),
        (Level::High, Level::Medium),
        (Level::Xhigh, Level::Medium),
        (Level::Max, Level::Medium),
    ];
    app.apply_agent_intent(crate::tui::update::intent::AgentIntent::RuntimePresentation(
        crate::tui::model::runtime_presentation::RuntimePresentationIntent::ReasoningCapability {
            effort_map: ladder_pairs.to_vec(),
        },
    ));
    // 当前档位 medium（映射后）。
    app.apply_agent_intent(
        crate::tui::update::intent::AgentIntent::RuntimePresentation(
            crate::tui::model::runtime_presentation::RuntimePresentationIntent::Thinking {
                enabled: true,
                level: Level::Medium,
            },
        ),
    );
    app.handle_slash_command("/think");
    let dialog = app
        .layout
        .active_dialog()
        .expect("/think 无参数应打开档位对话框");
    let options = dialog.options();
    assert_eq!(
        options[1], "minimal → off",
        "映射表给出降档时对话框必须如实标注（表驱动渲染），实际 {options:?}"
    );
    assert_eq!(options[2], "low → off", "实际 {options:?}");
    assert_eq!(
        options[4], "high → medium",
        "超上限档位应标注映射结果，实际 {options:?}"
    );
    assert_eq!(options[6], "max → medium", "实际 {options:?}");
    assert_eq!(
        options[3], "medium",
        "阶梯内档位无映射标注；当前档位行文案无字符标记（高亮由 Dialog
        current_index 渲染），实际 {options:?}"
    );
}

#[test]
fn think_dialog_confirm_resubmits_think_slash_command() {
    let mut app = app_with_builtin_router();
    app.handle_slash_command("/think");
    // 选中第 5 项（high）后确认：等价提交 /think high。
    for _ in 0..4 {
        app.layout.active_dialog_mut().unwrap().select_next();
    }
    let selected = app.layout.selected_model_key().expect("应有选中项");
    assert_eq!(selected, "high");

    let prefix = app
        .layout
        .dialog_command_prefix
        .clone()
        .expect("应记录命令前缀");
    assert_eq!(prefix, "/think");

    let command = format!("{prefix} {selected}");
    let result = app.handle_slash_command(&command);
    match sent_chat_event(&result) {
        Some(sdk::ChatInputEvent::SetThinking {
            request: sdk::ThinkingRequest::Level(level),
        }) => assert_eq!(*level, sdk::ReasoningLevel::High),
        other => panic!("对话框确认应等价 /think high，实际 {other:?}"),
    }
}
