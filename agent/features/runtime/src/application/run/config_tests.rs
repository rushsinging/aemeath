use super::RunConfigSnapshot;
use share::config::domain::snapshot::{ConfigRevision, ConfigSnapshot};
use share::config::{Config, PermissionModeConfig};

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
