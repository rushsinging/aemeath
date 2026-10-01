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
#[path = "concurrency_tests.rs"]
mod tests;
