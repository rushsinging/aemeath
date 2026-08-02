use crate::tui::adapter::agent_event::map_runtime_event;
use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityChangeKind, TuiActivityDetail, TuiActivityKind,
    TuiActivityObservation, TuiActivitySource, TuiActivityState, TuiActivityTiming, TuiHookPoint,
    TuiRuntimeEvent, UiActivityId,
};
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::interaction::{UiRunId, UiRunStepId};
use crate::tui::model::root::TuiModel;
use crate::tui::update::root_reducer::reduce_agent_event;

#[test]
fn hook_activity_scenario_reaches_tui_model_through_unique_activity_path() {
    let run_id = UiRunId::from("run-hook");
    let activity = TuiActivityObservation {
        id: UiActivityId::from("hook-activity"),
        run_id: run_id.clone(),
        run_step_id: Some(UiRunStepId::from("step-hook")),
        parent_activity_id: Some(UiActivityId::from("phase-activity")),
        source: TuiActivitySource::HookDispatch(UiActivityId::from("hook-source")),
        kind: TuiActivityKind::HookDispatch,
        state: TuiActivityState::Running,
        detail: TuiActivityDetail::Hook {
            point: TuiHookPoint::PreToolUse,
            script: "check-policy.sh".to_string(),
            attempt: 1,
        },
        audience: TuiActivityAudience::Operational,
        revision: 1,
        timing: TuiActivityTiming::default(),
    };

    let mapping = map_runtime_event(&TuiRuntimeEvent::ActivityChanged {
        kind: TuiActivityChangeKind::Started,
        activity,
    });
    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::ObserveActivityChange(_)]
    ));

    let mut model = TuiModel::default();
    reduce_agent_event(&mut model, mapping);
    let stored = model
        .conversation
        .activity_observations()
        .activities()
        .iter()
        .find(|activity| activity.run_id == run_id)
        .expect("hook activity stored");
    assert_eq!(stored.kind, TuiActivityKind::HookDispatch);
    assert_eq!(stored.audience, TuiActivityAudience::Operational);
}
