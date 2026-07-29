use super::RunConfigSnapshot;
use share::config::domain::snapshot::{ConfigRevision, ConfigSnapshot};
use share::config::{Config, PermissionModeConfig};

#[test]
fn captured_tool_selection_does_not_change_when_the_source_config_changes() {
    let mut config = Config::default();
    config.tools.disabled = vec!["AskUserQuestion".to_string()];
    let run = RunConfigSnapshot::capture(ConfigSnapshot::new(config));

    let later = ConfigSnapshot::new(Config::default());

    assert!(!run.tool_selection().allows("AskUserQuestion"));
    assert!(later.tool_selection().allows("AskUserQuestion"));
}

#[test]
fn captured_snapshot_keeps_revision_and_allow_all() {
    let mut config = Config::default();
    config.permissions.mode = PermissionModeConfig::AllowAll;
    let run = RunConfigSnapshot::capture(ConfigSnapshot::new_with_revision(
        ConfigRevision::new(7),
        config,
    ));

    assert_eq!(run.revision(), ConfigRevision::new(7));
    assert!(run.allow_all());
}
