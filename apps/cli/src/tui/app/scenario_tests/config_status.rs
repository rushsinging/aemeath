use super::super::testing::TuiScenarioHarness;

fn reload(permission_mode: &str) -> sdk::ChatEvent {
    sdk::ChatEvent::ConfigReloaded {
        event: sdk::ConfigReloadedEvent {
            changed_keys: vec!["permissions.mode".to_string()],
            scopes: vec![sdk::ConfigApplicationScopeView::Run],
            view: sdk::ConfigView {
                permission_mode: permission_mode.to_string(),
                ..Default::default()
            },
        },
    }
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
