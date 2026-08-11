use super::BoundaryHookPort;
use hook::HookPoint;

#[test]
fn boundary_binding_matches_the_complete_hook_point_classification_contract() {
    let expectations = [
        (HookPoint::PreToolUse, false),
        (HookPoint::UserPromptSubmit, true),
        (HookPoint::PreCompact, true),
        (HookPoint::PermissionRequest, true),
        (HookPoint::Elicitation, true),
        (HookPoint::UserPromptExpansion, true),
        (HookPoint::Stop, true),
        (HookPoint::PostToolUse, false),
        (HookPoint::PostToolUseFailure, false),
        (HookPoint::PostCompact, false),
        (HookPoint::PostToolBatch, false),
        (HookPoint::ElicitationResult, false),
        (HookPoint::SessionStart, true),
        (HookPoint::SessionEnd, true),
        (HookPoint::SubRunStart, true),
        (HookPoint::SubRunStop, true),
        (HookPoint::TaskCreated, false),
        (HookPoint::TaskCompleted, false),
        (HookPoint::Notification, false),
        (HookPoint::InstructionsLoaded, false),
        (HookPoint::StopFailure, false),
        (HookPoint::PermissionDenied, false),
        (HookPoint::ConfigChange, false),
        (HookPoint::CwdChanged, false),
        (HookPoint::FileChanged, false),
        (HookPoint::TeammateIdle, false),
    ];

    for (point, expected) in expectations {
        assert_eq!(
            BoundaryHookPort::allows(point),
            expected,
            "BoundaryOnly binding classification changed for {point:?}"
        );
    }
}
