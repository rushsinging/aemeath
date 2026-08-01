use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityDetail, TuiActivityKind, TuiActivityObservation,
    TuiActivityState, TuiModelStreamState, TuiRunPhaseKind, TuiRunPurpose,
};
use crate::tui::model::conversation::activity_observation::ActivityObservationModel;
use crate::tui::model::conversation::interaction::UiRunId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ActivitySummary {
    pub(crate) run_id: UiRunId,
    pub(crate) revision: u64,
    pub(crate) phase_text: String,
    pub(crate) total_elapsed_ms: u64,
    pub(crate) phase_elapsed_ms: u64,
    pub(crate) detail: Option<String>,
    pub(crate) invoking_model: bool,
}

pub(crate) struct ActivitySummaryAssembler;

impl ActivitySummaryAssembler {
    pub(crate) fn assemble(model: &ActivityObservationModel) -> Option<ActivitySummary> {
        let root = model
            .activities()
            .iter()
            .filter(|activity| is_live_main_root(activity))
            .max_by_key(|activity| activity.revision)?;
        if model.is_stale(&root.run_id) {
            return None;
        }

        let run_id = root.run_id.clone();
        let phase = model
            .activities()
            .iter()
            .filter(|activity| activity.run_id == run_id && is_live_phase(activity))
            .max_by_key(|activity| activity.revision);
        let leaf = model
            .activities()
            .iter()
            .filter(|activity| activity.run_id == run_id && is_visible_leaf(activity))
            .max_by_key(|activity| activity.revision);
        let primary = match (phase, leaf) {
            (Some(phase), Some(leaf)) if leaf.revision > phase.revision => leaf,
            (Some(phase), _) => phase,
            (None, Some(leaf)) => leaf,
            (None, None) => return None,
        };
        let phase_text = phase_label(primary)?;

        Some(ActivitySummary {
            revision: model.revision_for(&run_id).unwrap_or(root.revision),
            run_id,
            phase_text,
            total_elapsed_ms: root.timing.total_elapsed_ms,
            phase_elapsed_ms: primary.timing.state_elapsed_ms,
            detail: stable_detail(leaf),
            invoking_model: matches!(
                primary.detail,
                TuiActivityDetail::Model {
                    stream: TuiModelStreamState::Invoking
                        | TuiModelStreamState::WaitingForFirstToken
                        | TuiModelStreamState::Streaming
                        | TuiModelStreamState::Retrying,
                    ..
                }
            ),
        })
    }
}

fn is_live_main_root(activity: &TuiActivityObservation) -> bool {
    activity.kind == TuiActivityKind::Run
        && activity.parent_activity_id.is_none()
        && matches!(
            activity.state,
            TuiActivityState::Running | TuiActivityState::Waiting
        )
        && matches!(
            activity.detail,
            TuiActivityDetail::Run {
                purpose: TuiRunPurpose::Main
            }
        )
}

fn is_live_phase(activity: &TuiActivityObservation) -> bool {
    matches!(
        activity.state,
        TuiActivityState::Running | TuiActivityState::Waiting
    ) && matches!(activity.kind, TuiActivityKind::RunPhase(_))
}

fn is_visible_leaf(activity: &TuiActivityObservation) -> bool {
    activity.audience == TuiActivityAudience::User
        && matches!(
            activity.state,
            TuiActivityState::Running | TuiActivityState::Waiting
        )
        && !matches!(
            activity.kind,
            TuiActivityKind::Run | TuiActivityKind::RunPhase(_)
        )
}

fn phase_label(activity: &TuiActivityObservation) -> Option<String> {
    let label = match &activity.detail {
        TuiActivityDetail::Phase { phase } => match phase {
            TuiRunPhaseKind::DrainingInput => "Preparing input…",
            TuiRunPhaseKind::PreparingContext => "Preparing context…",
            TuiRunPhaseKind::ApplyingResponse => "Applying response…",
            TuiRunPhaseKind::AwaitingToolApproval => "Waiting for approval…",
            TuiRunPhaseKind::ExecutingTools => "Calling tools…",
            TuiRunPhaseKind::FinalizingStep => "Finalizing step…",
            TuiRunPhaseKind::CancellingStep => "Cancelling step…",
            TuiRunPhaseKind::Terminating => "Terminating…",
        },
        TuiActivityDetail::Model { .. } => "Thinking…",
        TuiActivityDetail::Tool { .. } => "Calling tools…",
        TuiActivityDetail::Hook { .. } => "Running hooks…",
        TuiActivityDetail::Compact { .. } => "Compacting…",
        TuiActivityDetail::Interaction { .. } => "Waiting for input…",
        TuiActivityDetail::ChildRun { .. } => "Running agent…",
        TuiActivityDetail::Run { .. } => return None,
    };
    Some(label.to_string())
}

fn stable_detail(activity: Option<&TuiActivityObservation>) -> Option<String> {
    let activity = activity?;
    if activity.timing.total_elapsed_ms < 500 {
        return None;
    }
    match &activity.detail {
        TuiActivityDetail::Tool {
            name,
            summary,
            parallel_count,
        } => {
            if *parallel_count > 1 {
                Some(format!("Running {parallel_count} tools"))
            } else {
                summary
                    .as_deref()
                    .filter(|summary| !summary.is_empty())
                    .map(|summary| format!("{name} {summary}"))
                    .or_else(|| Some(name.clone()))
            }
        }
        TuiActivityDetail::ChildRun { role, .. } => Some(format!("Running {role}")),
        _ => None,
    }
}
