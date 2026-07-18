use std::time::Duration;

use share::config::domain::merge::{apply_patch, AuditConfigPatch, ConfigPatch};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

#[test]
fn usage_worker_config_defaults_and_zero_values_normalize() {
    let default = ConfigSnapshot::new(Config::default()).usage_worker_config();
    assert_eq!(default.capacity(), 1024);
    assert_eq!(default.shutdown_timeout(), Duration::from_secs(5));

    let mut config = Config::default();
    config.audit.usage_queue_capacity = 0;
    config.audit.usage_shutdown_timeout_ms = 0;
    let normalized = ConfigSnapshot::new(config).usage_worker_config();
    assert_eq!(normalized.capacity(), 1024);
    assert_eq!(normalized.shutdown_timeout(), Duration::from_secs(5));
}

#[test]
fn audit_config_patch_merges_fields_independently() {
    let base = Config::default();
    let capacity_only = apply_patch(
        base,
        ConfigPatch {
            audit: Some(AuditConfigPatch {
                usage_queue_capacity: Some(8),
                usage_shutdown_timeout_ms: None,
            }),
            ..ConfigPatch::default()
        },
    );
    let merged = apply_patch(
        capacity_only,
        ConfigPatch {
            audit: Some(AuditConfigPatch {
                usage_queue_capacity: None,
                usage_shutdown_timeout_ms: Some(250),
            }),
            ..ConfigPatch::default()
        },
    );
    let value = ConfigSnapshot::new(merged).usage_worker_config();
    assert_eq!(value.capacity(), 8);
    assert_eq!(value.shutdown_timeout(), Duration::from_millis(250));
}
