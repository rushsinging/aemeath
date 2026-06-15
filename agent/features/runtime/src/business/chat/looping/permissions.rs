use crate::business::agent::ToolCall;
use share::tool::PolicyDecision;
use tools::api::ToolRegistry;

use super::engine::{DeniedCall, PolicyEngine};

/// Evaluate all tool calls through the PolicyEngine.
///
/// Returns `(approved, denied)`:
/// - `approved`: calls that passed policy, with **path fields normalised** to
///   absolute paths by the engine. The returned `ToolCall`s are owned clones
///   so downstream code can consume them without lifetime entanglement.
/// - `denied`: calls rejected by policy, each carrying a human-readable reason.
pub(crate) fn evaluate_calls(
    tool_calls: &[ToolCall],
    registry: &ToolRegistry,
    engine: &PolicyEngine,
) -> (Vec<ToolCall>, Vec<DeniedCall>) {
    let mut approved = Vec::with_capacity(tool_calls.len());
    let mut denied = Vec::new();

    for call in tool_calls {
        let decision = match registry.get(&call.name) {
            Some(tool) => engine.evaluate(&call.input, Some(tool.as_ref())),
            None => engine.evaluate(&call.input, None),
        };

        match decision {
            PolicyDecision::Allow(normalized_input) => {
                approved.push(ToolCall {
                    id: call.id.clone(),
                    provider_id: call.provider_id.clone(),
                    name: call.name.clone(),
                    index: call.index,
                    input: normalized_input,
                });
            }
            PolicyDecision::Deny { reason } => {
                denied.push(DeniedCall {
                    id: call.id.to_string(),
                    name: call.name.clone(),
                    reason,
                });
            }
        }
    }

    (approved, denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use tools::api::{Tool, ToolExecutionContext, ToolRegistry, ToolResult};

    fn call(name: &str, input: serde_json::Value) -> ToolCall {
        ToolCall {
            provider_id: "provider-test".to_string(),
            id: sdk::ids::ToolCallId::from_legacy_or_new(&format!("{name}-id")),
            name: name.to_string(),
            index: 0,
            input,
        }
    }

    /// Mock tool whose `is_input_safe` delegates to the real readonly-command
    /// checker, simulating BashTool's behaviour.
    struct MockBashTool;
    #[async_trait]
    impl Tool for MockBashTool {
        fn name(&self) -> &str {
            "Bash"
        }
        fn description(&self) -> &str {
            ""
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({})
        }
        fn is_read_only(&self) -> bool {
            false
        }
        fn is_input_safe(&self, input: &serde_json::Value) -> bool {
            input
                .get("command")
                .and_then(|v| v.as_str())
                .map(tools::api::is_readonly_command)
                .unwrap_or(false)
        }
        async fn call(
            &self,
            _: serde_json::Value,
            _: &ToolExecutionContext,
        ) -> ToolResult {
            unreachable!()
        }
    }

    #[test]
    fn test_evaluate_calls_allow_all_approves_everything() {
        let registry = ToolRegistry::new();
        let engine = PolicyEngine::new(
            std::path::Path::new("/tmp"),
            std::path::Path::new("/tmp"),
            true, // allow_all
        );
        let calls = vec![
            call("Edit", json!({})),
            call("Bash", json!({"command": "rm -rf x"})),
        ];

        let (approved, denied) = evaluate_calls(&calls, &registry, &engine);

        assert_eq!(approved.len(), 2);
        assert!(denied.is_empty());
    }

    #[test]
    fn test_evaluate_calls_readonly_bash_is_approved() {
        let registry = ToolRegistry::new();
        registry.register(Box::new(MockBashTool));
        let engine = PolicyEngine::new(
            std::path::Path::new("/tmp"),
            std::path::Path::new("/tmp"),
            false,
        );
        let calls = vec![call("Bash", json!({"command": "git status --short"}))];

        let (approved, denied) = evaluate_calls(&calls, &registry, &engine);

        assert_eq!(approved.len(), 1);
        assert!(denied.is_empty());
    }

    #[test]
    fn test_evaluate_calls_unknown_tool_is_denied() {
        let registry = ToolRegistry::new();
        let engine = PolicyEngine::new(
            std::path::Path::new("/tmp"),
            std::path::Path::new("/tmp"),
            false,
        );
        let calls = vec![call("UnknownTool", json!({}))];

        let (approved, denied) = evaluate_calls(&calls, &registry, &engine);

        assert!(approved.is_empty());
        assert_eq!(denied.len(), 1);
    }
}
