use crate::business::agent::ToolCall;
use tools::api::ToolRegistry;

pub(crate) fn split_approved_calls<'a>(
    tool_calls: &'a [ToolCall],
    registry: &ToolRegistry,
    allow_all: bool,
) -> (Vec<&'a ToolCall>, Vec<&'a ToolCall>) {
    if allow_all {
        return (tool_calls.iter().collect(), Vec::new());
    }

    tool_calls
        .iter()
        .partition(|call| is_auto_approved(call, registry))
}

fn is_auto_approved(call: &ToolCall, registry: &ToolRegistry) -> bool {
    if call.name == "Bash" {
        return call
            .input
            .get("command")
            .and_then(|v| v.as_str())
            .map(tools::api::is_readonly_command)
            .unwrap_or(false);
    }

    registry
        .get(&call.name)
        .map(|tool| tool.is_read_only())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tools::api::ToolRegistry;

    fn call(name: &str, input: serde_json::Value) -> ToolCall {
        ToolCall {
            id: format!("{name}-id"),
            name: name.to_string(),
            index: 0,
            input,
        }
    }

    #[test]
    fn test_split_approved_calls_allow_all_approves_everything() {
        let registry = ToolRegistry::new();
        let calls = vec![
            call("Edit", json!({})),
            call("Bash", json!({"command":"rm -rf x"})),
        ];

        let (approved, denied) = split_approved_calls(&calls, &registry, true);

        assert_eq!(approved.len(), 2);
        assert!(denied.is_empty());
    }

    #[test]
    fn test_split_approved_calls_readonly_bash_is_approved() {
        let registry = ToolRegistry::new();
        let calls = vec![call("Bash", json!({"command":"git status --short"}))];

        let (approved, denied) = split_approved_calls(&calls, &registry, false);

        assert_eq!(approved.len(), 1);
        assert!(denied.is_empty());
    }

    #[test]
    fn test_split_approved_calls_unknown_tool_is_denied() {
        let registry = ToolRegistry::new();
        let calls = vec![call("UnknownTool", json!({}))];

        let (approved, denied) = split_approved_calls(&calls, &registry, false);

        assert!(approved.is_empty());
        assert_eq!(denied.len(), 1);
    }
}
