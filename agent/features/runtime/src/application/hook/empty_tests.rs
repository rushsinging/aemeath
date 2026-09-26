use super::BoundaryHookPort;
use hook::HookPointData;

#[test]
fn boundary_binding_matches_the_complete_hook_point_classification_contract() {
    let expectations = [
        (HookPointData::PreToolUse, false),
        (HookPointData::UserPromptSubmit, true),
        (HookPointData::PreCompact, true),
        (HookPointData::PermissionRequest, true),
        (HookPointData::Elicitation, true),
        (HookPointData::UserPromptExpansion, true),
        (HookPointData::Stop, true),
        (HookPointData::PostToolUse, false),
        (HookPointData::PostToolUseFailure, false),
        (HookPointData::PostCompact, false),
        (HookPointData::PostToolBatch, false),
        (HookPointData::ElicitationResult, false),
        (HookPointData::SessionStart, true),
        (HookPointData::SessionEnd, true),
        (HookPointData::SubRunStart, true),
        (HookPointData::SubRunStop, true),
        (HookPointData::TaskCreated, false),
        (HookPointData::TaskCompleted, false),
        (HookPointData::Notification, false),
        (HookPointData::InstructionsLoaded, false),
        (HookPointData::StopFailure, false),
        (HookPointData::PermissionDenied, false),
        (HookPointData::ConfigChange, false),
        (HookPointData::CwdChanged, false),
        (HookPointData::FileChanged, false),
        (HookPointData::TeammateIdle, false),
    ];

    for (point, expected) in expectations {
        assert_eq!(
            BoundaryHookPort::allows(point),
            expected,
            "BoundaryOnly binding classification changed for {point:?}"
        );
    }
}
