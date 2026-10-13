use super::super::testing::TuiScenarioHarness;
use crate::tui::model::output_timeline::OutputTimelineItem;

fn system_notices(harness: &TuiScenarioHarness) -> Vec<String> {
    harness
        .app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            OutputTimelineItem::System { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn reload_with_scopes(
    permission_mode: &str,
    scopes: Vec<sdk::ConfigApplicationScopeView>,
) -> sdk::ChatEvent {
    sdk::ChatEvent::ConfigReloaded {
        event: sdk::ConfigReloadedEvent {
            changed_keys: scopes
                .iter()
                .map(|scope| match scope {
                    sdk::ConfigApplicationScopeView::Immediate => {
                        "config:scope:immediate".to_string()
                    }
                    sdk::ConfigApplicationScopeView::Run => "config:scope:run".to_string(),
                    sdk::ConfigApplicationScopeView::SessionRestartRequired => {
                        "config:scope:session_restart_required".to_string()
                    }
                })
                .collect(),
            scopes,
            view: sdk::ConfigView {
                permission_mode: permission_mode.to_string(),
                ..Default::default()
            },
        },
    }
}

fn reload(permission_mode: &str) -> sdk::ChatEvent {
    reload_with_scopes(permission_mode, vec![sdk::ConfigApplicationScopeView::Run])
}

#[test]
fn config_reload_updates_status_policy_from_committed_view() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.config_view.permission_mode = "ask".to_string();
    assert_eq!(
        harness
            .app
            .status_view_model()
            .runtime
            .context
            .permission_mode,
        "Ask"
    );

    harness.sdk_runtime_batch([reload("allow_all")]);
    assert_eq!(harness.app.config_view.permission_mode, "allow_all");
    assert_eq!(
        harness
            .app
            .status_view_model()
            .runtime
            .context
            .permission_mode,
        "AllowAll"
    );

    harness.sdk_runtime_batch([reload("auto_read")]);
    assert_eq!(harness.app.config_view.permission_mode, "auto_read");
    assert_eq!(
        harness
            .app
            .status_view_model()
            .runtime
            .context
            .permission_mode,
        "AutoRead"
    );
}

#[test]
fn config_reload_run_scope_shows_next_run_notice_and_refreshes_model_cache() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.session.cache_models(vec![sdk::ModelSummary {
        provider: "Volcengine".to_string(),
        id: "deepseek-v4.1-flash".to_string(),
        name: "DeepSeek V4.1 Flash (Ark)".to_string(),
        context_window: 128_000,
        max_tokens: 8_192,
    }]);
    let before_model = harness.app.session.current_model_display.clone();

    harness.sdk_runtime_batch([reload("ask")]);
    harness.render();

    let notices = system_notices(&harness);
    assert!(
        notices.iter().any(|text| text.contains("下次 Run 生效")),
        "run scope must show next-run notice, notices={notices:?}"
    );
    assert!(
        notices
            .iter()
            .all(|text| !text.contains("重启 Session 后生效")),
        "run-only reload must not claim session restart, notices={notices:?}"
    );
    assert!(
        harness.app.session.cached_models.is_none(),
        "run scope must invalidate cached models so /model refetches"
    );
    assert!(
        harness.effects().iter().any(|effect| {
            matches!(
                effect,
                crate::tui::effect::effect::Effect::SendChatInputEvent {
                    event: sdk::ChatInputEvent::ListModels
                }
            )
        }),
        "run scope must request ListModels refresh, effects={:?}",
        harness.effects()
    );
    assert_eq!(
        harness.app.session.current_model_display, before_model,
        "config reload must not switch the bound session model display"
    );
}

#[test]
fn model_list_after_run_reload_corrects_current_model_display_by_provider_id() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.session.current_model_display = "Volcengine/DeepSeek V4.1 Flash (Ark)".to_string();
    harness.app.session.cache_models(vec![sdk::ModelSummary {
        provider: "Volcengine".to_string(),
        id: "deepseek-v4.1-flash".to_string(),
        name: "DeepSeek V4.1 Flash (Ark)".to_string(),
        context_window: 128_000,
        max_tokens: 8_192,
    }]);
    harness.app.apply_agent_intent(
        crate::tui::update::intent::AgentIntent::RuntimePresentation(
            crate::tui::model::runtime_presentation::RuntimePresentationIntent::ProviderModel {
                provider: Some("Volcengine".to_string()),
                model_id: Some("Volcengine/DeepSeek V4.1 Flash (Ark)".to_string()),
            },
        ),
    );

    harness.sdk_runtime_batch([reload("ask")]);
    assert!(
        harness.app.session.cached_models.is_none(),
        "run reload must invalidate cache before ModelList refresh"
    );

    harness.sdk_runtime_batch([sdk::ChatEvent::ModelList {
        models: vec![sdk::ModelSummary {
            provider: "Volcengine".to_string(),
            id: "deepseek-v4.1-flash".to_string(),
            name: "DeepSeek V4.1 Flash".to_string(),
            context_window: 128_000,
            max_tokens: 8_192,
        }],
    }]);

    assert_eq!(
        harness.app.session.current_model_display, "Volcengine/DeepSeek V4.1 Flash",
        "ModelList must rewrite display from bound provider/id after name change"
    );
    assert_eq!(
        harness.app.model.runtime_presentation.model_id(),
        Some("Volcengine/DeepSeek V4.1 Flash"),
        "status-bar presentation must follow corrected display"
    );
}

#[test]
fn config_reload_session_restart_scope_shows_restart_notice_without_model_refresh() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.app.session.cache_models(vec![sdk::ModelSummary {
        provider: "OmniRoute".to_string(),
        id: "cursor-auto".to_string(),
        name: "Cursor Auto".to_string(),
        context_window: 200_000,
        max_tokens: 8_192,
    }]);

    harness.sdk_runtime_batch([reload_with_scopes(
        "ask",
        vec![sdk::ConfigApplicationScopeView::SessionRestartRequired],
    )]);
    harness.render();

    let notices = system_notices(&harness);
    assert!(
        notices
            .iter()
            .any(|text| text.contains("重启 Session 后生效")),
        "session_restart_required must show restart notice, notices={notices:?}"
    );
    assert!(
        notices.iter().all(|text| !text.contains("下次 Run 生效")),
        "session-restart-only reload must not claim next-run, notices={notices:?}"
    );
    assert!(
        harness.app.session.cached_models.is_some(),
        "session_restart_required alone must not invalidate model list cache"
    );
    assert!(
        !harness.effects().iter().any(|effect| {
            matches!(
                effect,
                crate::tui::effect::effect::Effect::SendChatInputEvent {
                    event: sdk::ChatInputEvent::ListModels
                }
            )
        }),
        "session_restart_required alone must not request ListModels"
    );
}
