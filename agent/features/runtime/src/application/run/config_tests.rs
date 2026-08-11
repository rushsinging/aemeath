use super::RunConfigSnapshot;
use share::config::domain::snapshot::ConfigRevision;
use share::config::{Config, PermissionModeConfig};

#[test]
fn run_config_captures_hook_runtime_limits_from_committed_snapshot() {
    let config = Config {
        hooks: serde_json::from_str(
            r#"{
                "max_attempts": 2,
                "max_stop_hook_blocks": 4
            }"#,
        )
        .unwrap(),
        ..share::config::Config::default()
    };

    let snapshot =
        RunConfigSnapshot::capture(share::config::domain::snapshot::ConfigSnapshot::new(config));

    assert_eq!(snapshot.hook_execution_policy().max_attempts(), 2);
    assert_eq!(snapshot.stop_hook_policy().max_blocks(), 4);
}

#[test]
fn captured_tool_selection_does_not_change_when_the_source_config_changes() {
    let mut config = Config::default();
    config.tools.disabled = vec!["AskUserQuestion".to_string()];
    let run =
        RunConfigSnapshot::capture(share::config::domain::snapshot::ConfigSnapshot::new(config));

    let later = share::config::domain::snapshot::ConfigSnapshot::new(Config::default());

    assert!(!run.tool_selection().allows("AskUserQuestion"));
    assert!(later.tool_selection().allows("AskUserQuestion"));
}

#[test]
fn captured_snapshot_keeps_revision_and_allow_all() {
    let mut config = Config::default();
    config.permissions.mode = PermissionModeConfig::AllowAll;
    let run = RunConfigSnapshot::capture(
        share::config::domain::snapshot::ConfigSnapshot::new_with_revision(
            ConfigRevision::new(7),
            config,
        ),
    );

    assert_eq!(run.revision(), ConfigRevision::new(7));
    assert!(run.allow_all());
}
