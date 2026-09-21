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
