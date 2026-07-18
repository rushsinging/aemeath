use share::config::domain::snapshot::{ConfigRevision, ConfigSnapshot};
use share::config::Config;

#[test]
fn config_snapshot_revision_is_monotonic_and_preserved_by_clone() {
    let first = ConfigSnapshot::new_with_revision(ConfigRevision::new(7), Config::default());
    let cloned = first.clone();
    let second = ConfigSnapshot::new_with_revision(first.revision().next(), Config::default());

    assert_eq!(first.revision(), ConfigRevision::new(7));
    assert_eq!(cloned.revision(), first.revision());
    assert_eq!(second.revision(), ConfigRevision::new(8));
}

#[test]
fn with_revision_shares_config_but_stamps_new_revision() {
    let mut config = Config::default();
    config.model.name = "test/model".to_string();
    let original = ConfigSnapshot::new_with_revision(ConfigRevision::new(3), config);
    let updated = original.with_revision(ConfigRevision::new(4));

    assert_eq!(updated.revision(), ConfigRevision::new(4));
    assert_eq!(original.revision(), ConfigRevision::new(3)); // original unchanged
                                                             // Same config content
    assert_eq!(updated.model_name(), "test/model");
    assert_eq!(original.model_name(), updated.model_name());
}

#[test]
fn to_config_returns_equivalent_owned_config() {
    let mut config = Config::default();
    config.model.name = "snapshot/model".to_string();
    config.model.context_size = 64000;
    let snapshot = ConfigSnapshot::new(config);

    let extracted = snapshot.to_config();
    assert_eq!(extracted.model.name, "snapshot/model");
    assert_eq!(extracted.model.context_size, 64000);
}
