pub fn resolve_concurrency_limits(
    cli_max_tool_concurrency: Option<usize>,
    cli_max_agent_concurrency: Option<usize>,
    snap_tool: usize,
    snap_agent: usize,
) -> (usize, usize) {
    let max_tool_concurrency = cli_max_tool_concurrency
        .filter(|&value| value > 0)
        .or_else(|| if snap_tool > 0 { Some(snap_tool) } else { None })
        .unwrap_or(10);
    let max_agent_concurrency = cli_max_agent_concurrency
        .filter(|&value| value > 0)
        .or_else(|| {
            if snap_agent > 0 {
                Some(snap_agent)
            } else {
                None
            }
        })
        .unwrap_or(4);

    (max_tool_concurrency, max_agent_concurrency)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_concurrency_limits_prefers_cli_values() {
        let result = resolve_concurrency_limits(Some(12), Some(6), 20, 8);

        assert_eq!(result, (12, 6));
    }

    #[test]
    fn test_resolve_concurrency_limits_uses_snap_when_cli_missing() {
        let result = resolve_concurrency_limits(None, None, 20, 8);

        assert_eq!(result, (20, 8));
    }

    #[test]
    fn test_resolve_concurrency_limits_uses_defaults_without_cli_or_snap() {
        let result = resolve_concurrency_limits(None, None, 0, 0);

        assert_eq!(result, (10, 4));
    }

    #[test]
    fn test_resolve_concurrency_limits_ignores_zero_and_uses_defaults() {
        let result = resolve_concurrency_limits(Some(0), Some(0), 0, 0);

        assert_eq!(result, (10, 4));
    }

    #[test]
    fn test_resolve_concurrency_limits_ignores_cli_zero_and_uses_snap() {
        let result = resolve_concurrency_limits(Some(0), Some(0), 20, 8);

        assert_eq!(result, (20, 8));
    }
}
