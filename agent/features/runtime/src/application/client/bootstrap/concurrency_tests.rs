use super::*;
use share::config::Config;

fn snapshot_with_concurrency(tool: usize, agent: usize) -> ConfigSnapshot {
    let mut config = Config::default();
    config.tools.max_concurrency = tool;
    config.agents.max_concurrency = agent;
    ConfigSnapshot::new(config)
}

#[test]
fn resolve_concurrency_limits_prefers_non_zero_cli_values() {
    let snapshot = snapshot_with_concurrency(20, 8);
    assert_eq!(
        resolve_concurrency_limits(Some(12), Some(6), &snapshot),
        (12, 6)
    );
}

#[test]
fn resolve_concurrency_limits_uses_snapshot_when_cli_missing() {
    let snapshot = snapshot_with_concurrency(20, 8);
    assert_eq!(resolve_concurrency_limits(None, None, &snapshot), (20, 8));
}

#[test]
fn resolve_concurrency_limits_ignores_cli_zero_and_uses_snapshot() {
    let snapshot = snapshot_with_concurrency(20, 8);
    assert_eq!(
        resolve_concurrency_limits(Some(0), Some(0), &snapshot),
        (20, 8)
    );
}

#[test]
fn resolve_concurrency_limits_uses_snapshot_normalized_defaults() {
    let snapshot = snapshot_with_concurrency(0, 0);
    assert_eq!(resolve_concurrency_limits(None, None, &snapshot), (10, 4));
}
