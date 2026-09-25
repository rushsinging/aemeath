use crate::adapters::{AllowAllPolicy, ConfiguredPolicy, StandardPolicy};
use crate::domain::{
    ApprovalSubject, PolicyDecision, PolicyMode, PolicyModeSource, PolicyPort, PolicyReason,
    PolicyRequest,
};
use sdk::ids::{RunId, RunStepId};
use share::config::PermissionModeConfig;
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
fn permission_mode_maps_to_single_policy_mode() {
    assert_eq!(
        PolicyMode::from(PermissionModeConfig::Ask),
        PolicyMode::Standard
    );
    assert_eq!(
        PolicyMode::from(PermissionModeConfig::AutoRead),
        PolicyMode::Standard
    );
    assert_eq!(
        PolicyMode::from(PermissionModeConfig::AllowAll),
        PolicyMode::AllowAll
    );
}

#[test]
fn allow_all_authorization_disables_every_authorization_guard() {
    assert_eq!(
        tools::AuthorizationContext::ALLOW_ALL,
        tools::AuthorizationContext {
            allow_outside_workspace: true,
            require_read_before_write: false,
            enforce_bash_safety: false,
            enforce_tool_fuse: false,
        }
    );
}

#[test]
fn standard_authorization_preserves_existing_guards() {
    assert_eq!(
        tools::AuthorizationContext::STANDARD,
        tools::AuthorizationContext {
            allow_outside_workspace: false,
            require_read_before_write: true,
            enforce_bash_safety: true,
            enforce_tool_fuse: true,
        }
    );
}

#[test]
fn standard_policy_returns_allow_with_standard_authorization() {
    let policy: &dyn PolicyPort = &StandardPolicy;
    assert_eq!(
        policy.evaluate(&request("Read", ToolCapability::Read)),
        PolicyDecision::Allow(tools::AuthorizationContext::STANDARD)
    );
}

#[derive(Clone)]
struct MutableModeSource(std::sync::Arc<std::sync::RwLock<PermissionModeConfig>>);

impl PolicyModeSource for MutableModeSource {
    fn current_mode(&self) -> PolicyMode {
        (*self.0.read().expect("mode source lock")).into()
    }
}

#[test]
fn configured_policy_reads_current_mode_for_every_evaluation() {
    let mode = std::sync::Arc::new(std::sync::RwLock::new(PermissionModeConfig::Ask));
    let policy = ConfiguredPolicy::new(MutableModeSource(mode.clone()));
    let request = request("Read", ToolCapability::Read);

    assert_eq!(
        policy.evaluate(&request),
        PolicyDecision::Allow(tools::AuthorizationContext::STANDARD)
    );

    *mode.write().expect("mode source lock") = PermissionModeConfig::AllowAll;

    assert_eq!(
        policy.evaluate(&request),
        PolicyDecision::Allow(tools::AuthorizationContext::ALLOW_ALL)
    );
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
        request("Read", ToolCapability::Read),
        request("Edit", ToolCapability::Write),
        request("Bash", ToolCapability::Execute),
    ] {
        assert_eq!(
            policy.evaluate(&request),
            PolicyDecision::Allow(tools::AuthorizationContext::ALLOW_ALL)
        );
    }
}

#[test]
fn policy_request_rejects_empty_workspace_root() {
    let result = PolicyRequest::new(
        RunId::new_v7(),
        RunStepId::new_v7(),
        ToolName::new("Read"),
        ToolCapabilities::Read,
        "",
    );
    assert!(result.is_err());
}
