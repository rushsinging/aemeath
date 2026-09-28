use crate::adapters::StandardPolicy;
use crate::domain::{
    ApprovalSubjectData, Policy, PolicyDecisionData, PolicyModeData, PolicyReasonData,
    PolicyRequestData,
};
use crate::{allow_all, configured};
use share::config::PermissionModeConfig;
use share::ids::{RunId, RunStepId};
use share::tools_vocab::{ToolCapabilities, ToolCapability, ToolName};

fn request(tool: &str, capability: ToolCapability) -> PolicyRequestData {
    PolicyRequestData::new(
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
        PolicyModeData::from(PermissionModeConfig::Ask),
        PolicyModeData::Standard
    );
    assert_eq!(
        PolicyModeData::from(PermissionModeConfig::AutoRead),
        PolicyModeData::Standard
    );
    assert_eq!(
        PolicyModeData::from(PermissionModeConfig::AllowAll),
        PolicyModeData::AllowAll
    );
}

#[test]
fn allow_all_authorization_disables_every_authorization_guard() {
    assert_eq!(
        share::tools_vocab::AuthorizationContext::ALLOW_ALL,
        share::tools_vocab::AuthorizationContext {
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
        share::tools_vocab::AuthorizationContext::STANDARD,
        share::tools_vocab::AuthorizationContext {
            allow_outside_workspace: false,
            require_read_before_write: true,
            enforce_bash_safety: true,
            enforce_tool_fuse: true,
        }
    );
}

#[test]
fn standard_policy_returns_allow_with_standard_authorization() {
    let policy: &dyn Policy = &StandardPolicy;
    assert_eq!(
        policy.evaluate(&request("Read", ToolCapability::Read)),
        PolicyDecisionData::Allow(share::tools_vocab::AuthorizationContext::STANDARD)
    );
}

#[test]
fn configured_policy_reads_current_mode_for_every_evaluation() {
    let mode = std::sync::Arc::new(std::sync::RwLock::new(PermissionModeConfig::Ask));
    let mode_for_closure = std::sync::Arc::clone(&mode);
    let policy = configured(move || (*mode_for_closure.read().expect("mode lock")).into());
    let request = request("Read", ToolCapability::Read);

    assert_eq!(
        policy.evaluate(&request),
        PolicyDecisionData::Allow(share::tools_vocab::AuthorizationContext::STANDARD)
    );

    *mode.write().expect("mode source lock") = PermissionModeConfig::AllowAll;

    assert_eq!(
        policy.evaluate(&request),
        PolicyDecisionData::Allow(share::tools_vocab::AuthorizationContext::ALLOW_ALL)
    );
}

#[test]
fn policy_decision_future_variants_keep_typed_reason_and_subject() {
    let deny = PolicyDecisionData::Deny {
        reason: PolicyReasonData::RestrictedTool,
    };
    let approval = PolicyDecisionData::RequireApproval {
        reason: PolicyReasonData::RestrictedWorkspace,
        subject: ApprovalSubjectData::UserInteraction,
    };
    assert!(matches!(deny, PolicyDecisionData::Deny { .. }));
    assert!(matches!(
        approval,
        PolicyDecisionData::RequireApproval { .. }
    ));
}

#[test]
fn allow_all_policy_contract_allows_every_valid_request() {
    let policy = allow_all();
    for request in [
        request("Read", ToolCapability::Read),
        request("Edit", ToolCapability::Write),
        request("Bash", ToolCapability::Execute),
    ] {
        assert_eq!(
            policy.evaluate(&request),
            PolicyDecisionData::Allow(share::tools_vocab::AuthorizationContext::ALLOW_ALL)
        );
    }
}

#[test]
fn policy_request_rejects_empty_workspace_root() {
    let result = PolicyRequestData::new(
        RunId::new_v7(),
        RunStepId::new_v7(),
        ToolName::new("Read"),
        ToolCapabilities::Read,
        "",
    );
    assert!(result.is_err());
}
