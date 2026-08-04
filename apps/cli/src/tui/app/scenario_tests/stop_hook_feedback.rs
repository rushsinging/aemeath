use crate::tui::adapter::agent_event::map_runtime_event;
use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiResumedSessionStep, TuiStopHookFeedback,
};
use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityChangeKind, TuiActivityDetail, TuiActivityKind,
    TuiActivityObservation, TuiActivitySource, TuiActivityState, TuiActivityTiming, TuiHookPoint,
    TuiRuntimeEvent, UiActivityId,
};
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::interaction::{UiRunId, UiRunStepId};
use crate::tui::model::root::TuiModel;
use crate::tui::update::root_reducer::reduce_agent_event;

use super::super::testing::TuiScenarioHarness;

#[test]
fn live_stop_hook_feedback_renders_once_with_complete_user_visible_detail() {
    let mut harness = TuiScenarioHarness::new(120, 30);
    harness.runtime_event(TuiRuntimeEvent::StopHookFeedback(TuiStopHookFeedback {
        summary: "Stop hook 阻止了停止。".to_string(),
        command: "check-agent-stop.sh".to_string(),
        exit_code: Some(2),
        reason: "exit code 2".to_string(),
        stdout_preview: "stdout preview".to_string(),
        stderr_preview: "stderr preview".to_string(),
        stdout_truncated: true,
        stderr_truncated: false,
        output_file: Some("/tmp/stop-hook.txt".to_string()),
    }));
    harness.render();

    let screen = harness.screen();
    assert_eq!(screen.matches("Stop hook").count(), 1, "{screen}");
    assert!(screen.contains("check-agent-stop.sh"), "{screen}");
    assert!(screen.contains("exit code 2"), "{screen}");
    assert!(screen.contains("stderr preview"), "{screen}");
}

#[test]
fn live_and_resumed_stop_hook_feedback_render_equivalent_notice() {
    let feedback = TuiStopHookFeedback {
        summary: "Stop hook 阻止了停止。".to_string(),
        command: "check-agent-stop.sh".to_string(),
        exit_code: Some(2),
        reason: "exit code 2".to_string(),
        stdout_preview: "stdout preview".to_string(),
        stderr_preview: "stderr preview".to_string(),
        stdout_truncated: false,
        stderr_truncated: false,
        output_file: None,
    };

    let mut live = TuiScenarioHarness::new(120, 30);
    live.runtime_event(TuiRuntimeEvent::StopHookFeedback(feedback.clone()));
    live.render();

    let mut resumed = TuiScenarioHarness::new(120, 30);
    resumed.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps: vec![TuiResumedSessionStep {
            run_id: "run-stop-hook".to_string(),
            step_id: "step-stop-hook".to_string(),
            messages: vec![TuiChatMessage::stop_hook_feedback(
                "LLM-only feedback",
                feedback,
            )],
            finalize_cause: None,
            duration_ms: None,
        }],
        display_history: None,
        session_id: "session-stop-hook".to_string(),
        created_at: 0,
        compacted: false,
    });
    resumed.render();

    for expected in ["Stop hook", "check-agent-stop.sh", "exit code 2"] {
        assert!(live.screen().contains(expected), "{}", live.screen());
        assert!(resumed.screen().contains(expected), "{}", resumed.screen());
    }
    assert!(!resumed.screen().contains("LLM-only feedback"));
}

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
