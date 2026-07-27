use super::{classify_application_scopes, ConfigApplicationScope};
use crate::config::hooks::{HookEntry, HookEvent};
use crate::config::ui::{
    ElementSpacingOverride, MarkdownSpacingMode, MarkdownSpacingOverrides, SpacingLines,
};
use crate::config::{Config, PermissionModeConfig};

fn changed(mut change: impl FnMut(&mut Config)) -> Vec<ConfigApplicationScope> {
    let before = Config::default();
    let mut after = Config::default();
    change(&mut after);
    classify_application_scopes(&before, &after)
}

#[test]
fn allow_all_is_run_scoped() {
    let scopes = changed(|config| {
        config.permissions.mode = PermissionModeConfig::AllowAll;
    });

    assert_eq!(scopes, vec![ConfigApplicationScope::Run]);
}

#[test]
fn tui_is_session_restart_required() {
    let scopes = changed(|config| config.ui.tui = false);

    assert_eq!(scopes, vec![ConfigApplicationScope::SessionRestartRequired]);
}

#[test]
fn provider_and_hooks_are_run_scoped() {
    let scopes = changed(|config| {
        config.api.base_url = Some("https://example.test".to_string());
        config.hooks.events.insert(
            HookEvent::Stop,
            vec![HookEntry {
                matcher: String::new(),
                command: "true".to_string(),
                timeout: 1,
            }],
        );
    });

    assert_eq!(scopes, vec![ConfigApplicationScope::Run]);
}

#[test]
fn user_agent_change_is_run_scoped() {
    let scopes = changed(|config| config.api.user_agent = "aemeath-test/1.0".into());

    assert_eq!(scopes, vec![ConfigApplicationScope::Run]);
}

#[test]
fn unchanged_config_has_no_application_scope() {
    assert!(classify_application_scopes(&Config::default(), &Config::default()).is_empty());
}

#[test]
fn markdown_spacing_changes_are_immediate_without_misclassifying_other_ui_fields() {
    let mode_scopes = changed(|config| config.ui.markdown_spacing = MarkdownSpacingMode::Compact);
    assert_eq!(mode_scopes, vec![ConfigApplicationScope::Immediate]);

    let override_scopes = changed(|config| {
        config.ui.markdown_spacing_overrides = MarkdownSpacingOverrides {
            heading: Some(ElementSpacingOverride {
                before: Some(SpacingLines::new(1).unwrap()),
                after: None,
            }),
            ..Default::default()
        };
    });
    assert_eq!(override_scopes, vec![ConfigApplicationScope::Immediate]);

    let unrelated_ui_scopes = changed(|config| config.ui.progress = false);
    assert!(unrelated_ui_scopes.is_empty());
}

#[test]
fn scopes_are_stably_deduplicated() {
    let scopes = changed(|config| {
        config.ui.tui = false;
        config.storage.sessions_dir = Some("sessions".into());
        config.permissions.mode = PermissionModeConfig::AllowAll;
        config.tools.max_concurrency = 2;
    });

    assert_eq!(
        scopes,
        vec![
            ConfigApplicationScope::SessionRestartRequired,
            ConfigApplicationScope::Run,
        ]
    );
}
