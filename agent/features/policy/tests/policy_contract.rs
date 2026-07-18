use policy::{
    AllowAllPolicy, ApprovalSubject, PolicyDecision, PolicyMode, PolicyPort, PolicyReason,
    PolicyRequest,
};
use sdk::ids::{RunId, RunStepId};
use tools::{ToolCapabilities, ToolCapability, ToolName};

fn request(tool: &str, capability: ToolCapability) -> PolicyRequest {
    PolicyRequest::new(
        RunId::new_v7(),
        RunStepId::new_v7(),
        ToolName::new(tool),
        ToolCapabilities::single(capability),
        "/workspace",
    )
    .expect("valid request")
}

#[test]
fn policy_mode_v010_only_exposes_allow_all() {
    assert_eq!(PolicyMode::default(), PolicyMode::AllowAll);
}

#[test]
fn policy_decision_future_variants_keep_typed_reason_and_subject() {
    let deny = PolicyDecision::Deny {
        reason: PolicyReason::RestrictedTool,
    };
    let approval = PolicyDecision::RequireApproval {
        reason: PolicyReason::RestrictedWorkspace,
        subject: ApprovalSubject::UserInteraction,
    };
    assert!(matches!(deny, PolicyDecision::Deny { .. }));
    assert!(matches!(approval, PolicyDecision::RequireApproval { .. }));
}

#[test]
fn allow_all_policy_contract_allows_every_valid_request() {
    let policy: &dyn PolicyPort = &AllowAllPolicy;
    for request in [
        request("Read", ToolCapability::ReadWorkspace),
        request("Edit", ToolCapability::WriteWorkspace),
        request("Bash", ToolCapability::ExecuteProcess),
    ] {
        assert_eq!(policy.evaluate(&request), PolicyDecision::Allow);
    }
}

#[test]
fn policy_request_rejects_empty_workspace_root() {
    let result = PolicyRequest::new(
        RunId::new_v7(),
        RunStepId::new_v7(),
        ToolName::new("Read"),
        ToolCapabilities::ReadWorkspace,
        "",
    );
    assert!(result.is_err());
}
