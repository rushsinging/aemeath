use share::config::Config;

pub fn resolve_concurrency_limits(
    cli_max_tool_concurrency: Option<usize>,
    cli_max_agent_concurrency: Option<usize>,
    config_file: Option<&Config>,
) -> (usize, usize) {
    let max_tool_concurrency = cli_max_tool_concurrency
        .filter(|&value| value > 0)
        .or_else(|| {
            config_file
                .map(|config| config.tools.max_concurrency)
                .filter(|&value| value > 0)
        })
        .unwrap_or(10);
    let max_agent_concurrency = cli_max_agent_concurrency
        .filter(|&value| value > 0)
        .or_else(|| {
            config_file
                .map(|config| config.agents.max_concurrency)
                .filter(|&value| value > 0)
        })
        .unwrap_or(4);

    (max_tool_concurrency, max_agent_concurrency)
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::config::{AgentsConfig, Config, ToolsConfig};

    fn config_with_concurrency(tool: usize, agent: usize) -> Config {
        Config {
            tools: ToolsConfig {
                max_concurrency: tool,
                ..Default::default()
            },
            agents: AgentsConfig {
                max_concurrency: agent,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_resolve_concurrency_limits_prefers_cli_values() {
        let config = config_with_concurrency(20, 8);

        let result = resolve_concurrency_limits(Some(12), Some(6), Some(&config));

        assert_eq!(result, (12, 6));
    }

    #[test]
    fn test_resolve_concurrency_limits_uses_config_when_cli_missing() {
        let config = config_with_concurrency(20, 8);

        let result = resolve_concurrency_limits(None, None, Some(&config));

        assert_eq!(result, (20, 8));
    }

    #[test]
    fn test_resolve_concurrency_limits_uses_defaults_without_cli_or_config() {
        let result = resolve_concurrency_limits(None, None, None);

        assert_eq!(result, (10, 4));
    }

    #[test]
    fn test_resolve_concurrency_limits_ignores_zero_and_uses_defaults() {
        let config = config_with_concurrency(0, 0);

        let result = resolve_concurrency_limits(Some(0), Some(0), Some(&config));

        assert_eq!(result, (10, 4));
    }

    #[test]
    fn test_resolve_concurrency_limits_ignores_cli_zero_and_uses_config() {
        let config = config_with_concurrency(20, 8);

        let result = resolve_concurrency_limits(Some(0), Some(0), Some(&config));

        assert_eq!(result, (20, 8));
    }
}
