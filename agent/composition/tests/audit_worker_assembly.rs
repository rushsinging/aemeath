use std::time::Duration;

use composition::audit::usage_worker_config_from_snapshot;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

#[test]
fn composition_extracts_usage_worker_config_by_value() {
    let mut config = Config::default();
    config.audit.usage_queue_capacity = 17;
    config.audit.usage_shutdown_timeout_ms = 321;
    let snapshot = ConfigSnapshot::new(config);

    let value = usage_worker_config_from_snapshot(&snapshot);
    assert_eq!(value.capacity(), 17);
    assert_eq!(value.shutdown_timeout(), Duration::from_millis(321));
}
