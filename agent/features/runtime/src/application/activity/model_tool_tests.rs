use super::coordinator::{ActivityClock, ActivityIdSource};
use super::ActivityCoordinator;
use crate::application::tool::agent::ToolCall;
use sdk::{
    ActivityAudienceView, ActivityDetailView, ActivityKindView, ActivityStateView,
    CompactStageView, HookPointView, InteractionKindView, InteractionRequestId, ModelInvocationId,
    ModelStreamStateView, RunId, RunStepId, ToolCallId,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FixedClock(Arc<Mutex<u64>>);

impl ActivityClock for FixedClock {
    fn now_monotonic_ms(&self) -> u64 {
        *self.0.lock().expect("clock lock")
    }

    fn now_unix_ms(&self) -> u64 {
        self.now_monotonic_ms()
    }
}

#[derive(Default)]
struct FixedIds(Mutex<u64>);

impl ActivityIdSource for FixedIds {
    fn next_activity_id(&self) -> sdk::ActivityId {
        let mut next = self.0.lock().expect("id lock");
        *next += 1;
        sdk::ActivityId::new(format!("leaf-{next}"))
    }
}

fn coordinator() -> ActivityCoordinator {
    ActivityCoordinator::new(
        RunId::new("leaf-run"),
        Arc::new(FixedClock(Arc::new(Mutex::new(1_000)))),
        Arc::new(FixedIds::default()),
    )
}

fn root_and_phase(coordinator: &ActivityCoordinator, step_id: &RunStepId) -> sdk::ActivityId {
    let root_id = coordinator
        .start(super::StartActivity {
            run_step_id: None,
            parent_activity_id: None,
            source: super::ActivitySource::Run,
            kind: super::ActivityKind::Run,
            detail: super::ActivityDetail::Run,
            audience: sdk::ActivityAudienceView::User,
        })
        .expect("start root");
    coordinator
        .start(super::StartActivity {
            run_step_id: Some(step_id.clone()),
            parent_activity_id: Some(root_id),
            source: super::ActivitySource::RunStep(step_id.clone()),
            kind: super::ActivityKind::RunPhase(super::model::RunPhaseKind::ExecutingTools),
            detail: super::ActivityDetail::Phase(super::model::RunPhaseKind::ExecutingTools),
            audience: sdk::ActivityAudienceView::User,
        })
        .expect("start phase")
}

#[test]
fn model_activity_preserves_identity_retry_detail_and_terminal() {
    let coordinator = coordinator();
    let step_id = RunStepId::new("model-step");
    let parent_id = root_and_phase(&coordinator, &step_id);
    let invocation_id = ModelInvocationId::new("invocation-1");
    let activity_id = coordinator
        .start_model_invocation(
            step_id,
            parent_id,
            invocation_id,
            "test-model".to_string(),
            1,
        )
        .expect("start model");
    coordinator
        .update_model_invocation(
            activity_id.clone(),
            "test-model".to_string(),
            2,
            ModelStreamStateView::Retrying,
        )
        .expect("retry model");
    coordinator
        .finish(activity_id.clone(), super::ActivityTerminal::Succeeded)
        .expect("finish model");

    let activity = coordinator.snapshot().find(&activity_id).unwrap().clone();
    assert_eq!(activity.kind, ActivityKindView::ModelInvocation);
    assert_eq!(activity.state, ActivityStateView::Succeeded);
    assert_eq!(
        activity.detail,
        ActivityDetailView::Model {
            model: "test-model".to_string(),
            attempt: 2,
            stream: ModelStreamStateView::Retrying,
        }
    );
}

#[test]
fn hook_compaction_and_interaction_activities_preserve_typed_detail_and_lifecycle() {
    let coordinator = coordinator();
    let step_id = RunStepId::new("remaining-step");
    let parent_id = root_and_phase(&coordinator, &step_id);

    let hook_id = coordinator
        .start_hook_dispatch(step_id.clone(), parent_id.clone(), HookPointView::Stop, 1)
        .expect("start hook");
    coordinator
        .finish(hook_id.clone(), super::ActivityTerminal::Failed)
        .expect("finish hook");

    let compact_id = coordinator
        .start_compaction(
            step_id.clone(),
            parent_id.clone(),
            CompactStageView::Preparing,
        )
        .expect("start compaction");
    coordinator
        .update_compaction(
            compact_id.clone(),
            CompactStageView::Summarizing,
            Some(2),
            Some(4),
        )
        .expect("update compaction");
    coordinator
        .finish(compact_id.clone(), super::ActivityTerminal::Succeeded)
        .expect("finish compaction");

    let request_id = InteractionRequestId::new("interaction-1");
    let interaction_id = coordinator
        .start_interaction(
            step_id,
            parent_id,
            request_id,
            InteractionKindView::UserQuestion,
        )
        .expect("start interaction");
    coordinator
        .wait(super::UpdateActivity {
            activity_id: interaction_id.clone(),
            detail: None,
        })
        .expect("wait interaction");
    coordinator
        .resume(super::UpdateActivity {
            activity_id: interaction_id.clone(),
            detail: None,
        })
        .expect("resume interaction");
    coordinator
        .finish(interaction_id.clone(), super::ActivityTerminal::Cancelled)
        .expect("finish interaction");

    let snapshot = coordinator.snapshot();
    let hook = snapshot.find(&hook_id).expect("hook activity");
    assert_eq!(hook.kind, ActivityKindView::HookDispatch);
    assert_eq!(hook.state, ActivityStateView::Failed);
    assert_eq!(
        hook.detail,
        ActivityDetailView::Hook {
            point: HookPointView::Stop,
            attempt: 1,
        }
    );

    let compact = snapshot.find(&compact_id).expect("compact activity");
    assert_eq!(compact.kind, ActivityKindView::Compaction);
    assert_eq!(compact.state, ActivityStateView::Succeeded);
    assert_eq!(
        compact.detail,
        ActivityDetailView::Compact {
            stage: CompactStageView::Summarizing,
            current: Some(2),
            total: Some(4),
        }
    );

    let interaction = snapshot
        .find(&interaction_id)
        .expect("interaction activity");
    assert_eq!(interaction.kind, ActivityKindView::Interaction);
    assert_eq!(interaction.state, ActivityStateView::Cancelled);
    assert_eq!(
        interaction.detail,
        ActivityDetailView::Interaction {
            kind: InteractionKindView::UserQuestion,
        }
    );
    assert_eq!(interaction.audience, ActivityAudienceView::User);
}

#[test]
fn parallel_tools_keep_independent_identity_and_parallel_count() {
    let coordinator = coordinator();
    let step_id = RunStepId::new("tool-step");
    let parent_id = root_and_phase(&coordinator, &step_id);
    let first = ToolCall {
        id: ToolCallId::new("tool-1"),
        provider_id: "provider-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        input: serde_json::json!({}),
    };
    let second = ToolCall {
        id: ToolCallId::new("tool-2"),
        provider_id: "provider-2".to_string(),
        name: "Grep".to_string(),
        index: 1,
        input: serde_json::json!({}),
    };

    let first_id = coordinator
        .start_tool_call(step_id.clone(), parent_id.clone(), &first, 2)
        .expect("start first tool");
    let second_id = coordinator
        .start_tool_call(step_id, parent_id, &second, 2)
        .expect("start second tool");

    assert_ne!(first_id, second_id);
    let snapshot = coordinator.snapshot();
    for activity_id in [first_id, second_id] {
        let activity = snapshot.find(&activity_id).expect("tool activity");
        assert_eq!(activity.kind, ActivityKindView::ToolCall);
        assert!(matches!(
            &activity.detail,
            ActivityDetailView::Tool {
                parallel_count: 2,
                ..
            }
        ));
    }
}
