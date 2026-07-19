#[cfg(test)]
mod tests {
    use crate::application::agent::ToolCall;
    use crate::application::chat::looping::permissions::evaluate_calls;
    use policy::{ApprovalSubject, PolicyDecision, PolicyPort, PolicyReason, PolicyRequest};
    use std::sync::Mutex;
    use tools::ToolRegistry;

    struct RecordingPolicy {
        seen: Mutex<Vec<(sdk::RunId, sdk::RunStepId, tools::ToolName)>>,
        decision: PolicyDecision,
    }

    impl PolicyPort for RecordingPolicy {
        fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
            self.seen.lock().unwrap().push((
                request.run_id().clone(),
                request.run_step_id().clone(),
                request.tool_name().clone(),
            ));
            self.decision.clone()
        }
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            provider_id: format!("provider-{name}"),
            id: sdk::ToolCallId::new_v7(),
            name: name.into(),
            index: 0,
            input: serde_json::json!({}),
        }
    }

    #[test]
    fn evaluate_calls_uses_injected_policy_and_real_ids() {
        let registry = ToolRegistry::new();
        registry.declare_capabilities_for_test(
            &tools::ToolName::new("Read"),
            tools::ToolCapabilities::ReadWorkspace,
        );
        let run_id = sdk::RunId::new_v7();
        let step_id = sdk::RunStepId::new_v7();
        let policy = RecordingPolicy {
            seen: Mutex::new(Vec::new()),
            decision: PolicyDecision::Allow,
        };
        let call = call("Read");

        let (approved, denied) = evaluate_calls(
            &[call],
            &registry,
            &policy,
            &run_id,
            &step_id,
            std::path::Path::new("/workspace"),
        );

        assert_eq!(approved.len(), 1);
        assert!(denied.is_empty());
        let seen = policy.seen.lock().unwrap();
        assert_eq!(seen[0].0, run_id);
        assert_eq!(seen[0].1, step_id);
        assert_eq!(seen[0].2, tools::ToolName::new("Read"));
    }

    #[test]
    fn evaluate_calls_classifies_non_allow_decision_without_executing_tool() {
        let registry = ToolRegistry::new();
        registry.declare_capabilities_for_test(
            &tools::ToolName::new("Edit"),
            tools::ToolCapabilities::WriteWorkspace,
        );
        let policy = RecordingPolicy {
            seen: Mutex::new(Vec::new()),
            decision: PolicyDecision::Deny {
                reason: PolicyReason::RestrictedTool,
            },
        };
        let call = call("Edit");
        let (approved, denied) = evaluate_calls(
            &[call],
            &registry,
            &policy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            std::path::Path::new("/workspace"),
        );
        assert!(approved.is_empty());
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].name, "Edit");
        assert!(denied[0].reason.contains("RestrictedTool"));
    }

    #[test]
    fn evaluate_calls_maps_require_approval_to_denied_call_with_subject_and_reason() {
        let registry = ToolRegistry::new();
        registry.declare_capabilities_for_test(
            &tools::ToolName::new("Bash"),
            tools::ToolCapabilities::ExecuteProcess,
        );
        let policy = RecordingPolicy {
            seen: Mutex::new(Vec::new()),
            decision: PolicyDecision::RequireApproval {
                reason: PolicyReason::RestrictedWorkspace,
                subject: ApprovalSubject::UserInteraction,
            },
        };

        let (approved, denied) = evaluate_calls(
            &[call("Bash")],
            &registry,
            &policy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            std::path::Path::new("/workspace"),
        );

        assert!(approved.is_empty());
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].name, "Bash");
        assert_eq!(
            denied[0].reason,
            "approval required: UserInteraction: RestrictedWorkspace"
        );
    }

    #[test]
    fn evaluate_calls_rejects_unregistered_tool_without_invoking_policy() {
        let registry = ToolRegistry::new();
        let policy = RecordingPolicy {
            seen: Mutex::new(Vec::new()),
            decision: PolicyDecision::Allow,
        };

        let (approved, denied) = evaluate_calls(
            &[call("Unknown")],
            &registry,
            &policy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            std::path::Path::new("/workspace"),
        );

        assert!(approved.is_empty());
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].name, "Unknown");
        assert_eq!(
            denied[0].reason,
            "Tool is not registered with declared capabilities"
        );
        assert!(policy.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn evaluate_calls_rejects_invalid_policy_request_without_invoking_policy() {
        let registry = ToolRegistry::new();
        registry.declare_capabilities_for_test(
            &tools::ToolName::new("Read"),
            tools::ToolCapabilities::ReadWorkspace,
        );
        let policy = RecordingPolicy {
            seen: Mutex::new(Vec::new()),
            decision: PolicyDecision::Allow,
        };

        let (approved, denied) = evaluate_calls(
            &[call("Read")],
            &registry,
            &policy,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            std::path::Path::new(""),
        );

        assert!(approved.is_empty());
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].name, "Read");
        assert_eq!(denied[0].reason, "Policy 请求的工作区根不能为空");
        assert!(policy.seen.lock().unwrap().is_empty());
    }
}
