use share::config::domain::snapshot::ConfigSnapshot;

pub fn resolve_concurrency_limits(
    cli_max_tool_concurrency: Option<usize>,
    cli_max_agent_concurrency: Option<usize>,
    snapshot: &ConfigSnapshot,
) -> (usize, usize) {
    let max_tool_concurrency = cli_max_tool_concurrency
        .filter(|&value| value > 0)
        .unwrap_or_else(|| snapshot.max_tool_concurrency());
    let max_agent_concurrency = cli_max_agent_concurrency
        .filter(|&value| value > 0)
        .unwrap_or_else(|| snapshot.max_agent_concurrency());

    (max_tool_concurrency, max_agent_concurrency)
}

#[cfg(test)]
mod tests {
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

        let result = resolve_concurrency_limits(Some(12), Some(6), &snapshot);

        assert_eq!(result, (12, 6));
    }

    #[test]
    fn resolve_concurrency_limits_uses_snapshot_when_cli_missing() {
        let snapshot = snapshot_with_concurrency(20, 8);

        let result = resolve_concurrency_limits(None, None, &snapshot);

        assert_eq!(result, (20, 8));
    }

    #[test]
    fn resolve_concurrency_limits_ignores_cli_zero_and_uses_snapshot() {
        let snapshot = snapshot_with_concurrency(20, 8);

        let result = resolve_concurrency_limits(Some(0), Some(0), &snapshot);

        assert_eq!(result, (20, 8));
    }

    #[test]
    fn resolve_concurrency_limits_uses_snapshot_normalized_defaults() {
        let snapshot = snapshot_with_concurrency(0, 0);

        let result = resolve_concurrency_limits(None, None, &snapshot);

        assert_eq!(result, (10, 4));
    }
}
