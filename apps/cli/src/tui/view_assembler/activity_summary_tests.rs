use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityDetail, TuiActivityKind, TuiActivityObservation,
    TuiActivitySource, TuiActivityState, TuiActivityTiming, TuiHookPoint, TuiRunPhaseKind,
    TuiRunPurpose, UiActivityId,
};
use crate::tui::model::conversation::activity_observation::ActivityObservationModel;
use crate::tui::model::conversation::interaction::{UiRunId, UiRunStepId};
use crate::tui::view_assembler::activity_summary::ActivitySummaryAssembler;

fn activity(
    id: &str,
    revision: u64,
    kind: TuiActivityKind,
    state: TuiActivityState,
    detail: TuiActivityDetail,
    audience: TuiActivityAudience,
) -> TuiActivityObservation {
    TuiActivityObservation {
        id: UiActivityId::from(id),
        run_id: UiRunId::from("run"),
        run_step_id: Some(UiRunStepId::from("step")),
        parent_activity_id: None,
        source: match kind {
            TuiActivityKind::Run => TuiActivitySource::Run,
            TuiActivityKind::RunPhase(_) => TuiActivitySource::RunStep(UiRunStepId::from("step")),
            TuiActivityKind::HookDispatch => {
                TuiActivitySource::HookDispatch(UiActivityId::from(id))
            }
            _ => TuiActivitySource::Interaction(id.to_string()),
        },
        kind,
        state,
        detail,
        audience,
        revision,
        timing: TuiActivityTiming {
            total_elapsed_ms: 600,
            active_elapsed_ms: 600,
            state_elapsed_ms: 600,
            ..TuiActivityTiming::default()
        },
    }
}

fn model_with_leaf(leaf: TuiActivityObservation) -> ActivityObservationModel {
    let mut model = ActivityObservationModel::default();
    model.replace_for_test(
        UiRunId::from("run"),
        leaf.revision,
        vec![
            activity(
                "run-root",
                1,
                TuiActivityKind::Run,
                TuiActivityState::Running,
                TuiActivityDetail::Run {
                    purpose: TuiRunPurpose::Main,
                },
                TuiActivityAudience::User,
            ),
            activity(
                "phase",
                2,
                TuiActivityKind::RunPhase(TuiRunPhaseKind::ExecutingTools),
                TuiActivityState::Running,
                TuiActivityDetail::Phase {
                    phase: TuiRunPhaseKind::ExecutingTools,
                },
                TuiActivityAudience::User,
            ),
            leaf,
        ],
    );
    model
}

#[test]
fn operational_running_and_waiting_hooks_are_visible_without_opening_other_operational_activities()
{
    let hook = activity(
        "hook",
        3,
        TuiActivityKind::HookDispatch,
        TuiActivityState::Running,
        TuiActivityDetail::Hook {
            point: TuiHookPoint::PreToolUse,
            script: "check-policy.sh".to_string(),
            attempt: 1,
        },
        TuiActivityAudience::Operational,
    );
    let summary = ActivitySummaryAssembler::assemble(&model_with_leaf(hook)).expect("summary");
    assert_eq!(summary.phase_text, "PreToolUse · check-policy.sh");

    let waiting_hook = activity(
        "hook-waiting",
        3,
        TuiActivityKind::HookDispatch,
        TuiActivityState::Waiting,
        TuiActivityDetail::Hook {
            point: TuiHookPoint::PermissionRequest,
            script: "check-permission.sh".to_string(),
            attempt: 1,
        },
        TuiActivityAudience::Operational,
    );
    let summary =
        ActivitySummaryAssembler::assemble(&model_with_leaf(waiting_hook)).expect("summary");
    assert_eq!(
        summary.phase_text,
        "PermissionRequest · check-permission.sh"
    );

    let operational_interaction = activity(
        "interaction",
        3,
        TuiActivityKind::Interaction,
        TuiActivityState::Running,
        TuiActivityDetail::Interaction {
            kind: crate::tui::adapter::tui_runtime_event::TuiInteractionKind::UserQuestion,
        },
        TuiActivityAudience::Operational,
    );
    let summary = ActivitySummaryAssembler::assemble(&model_with_leaf(operational_interaction))
        .expect("phase summary remains");
    assert_eq!(summary.phase_text, "Calling tools…");
}

#[test]
fn failed_hook_remains_visible_but_fast_success_does_not_pollute_status() {
    let failed_hook = activity(
        "hook-failed",
        3,
        TuiActivityKind::HookDispatch,
        TuiActivityState::Failed,
        TuiActivityDetail::Hook {
            point: TuiHookPoint::Stop,
            script: "check-agent-stop.sh".to_string(),
            attempt: 3,
        },
        TuiActivityAudience::Operational,
    );
    let summary =
        ActivitySummaryAssembler::assemble(&model_with_leaf(failed_hook)).expect("summary");
    assert_eq!(summary.phase_text, "Stop failed · check-agent-stop.sh");

    let succeeded_hook = activity(
        "hook-succeeded",
        3,
        TuiActivityKind::HookDispatch,
        TuiActivityState::Succeeded,
        TuiActivityDetail::Hook {
            point: TuiHookPoint::Stop,
            script: "check-agent-stop.sh".to_string(),
            attempt: 1,
        },
        TuiActivityAudience::Operational,
    );
    let summary = ActivitySummaryAssembler::assemble(&model_with_leaf(succeeded_hook))
        .expect("phase summary remains");
    assert_eq!(summary.phase_text, "Calling tools…");
}
