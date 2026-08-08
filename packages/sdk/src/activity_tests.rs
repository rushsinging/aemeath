use crate::{
    ActivityAudienceView, ActivityChangeKind, ActivityDetailView, ActivityId, ActivityKindView,
    ActivitySnapshotView, ActivitySourceView, ActivityStateView, ActivityTimingView, ActivityView,
    ChatEvent, CompactStageView, CompactWorkView, HookPointView, InteractionKindView,
    ModelStreamStateView, RunPhaseKindView, RunPurposeView,
};

fn activity_fixture() -> ActivityView {
    ActivityView {
        id: ActivityId::new("activity-1"),
        run_id: crate::RunId::new("run-1"),
        run_step_id: Some(crate::RunStepId::new("step-1")),
        parent_activity_id: Some(ActivityId::new("root-activity")),
        source: ActivitySourceView::ModelInvocation(crate::ModelInvocationId::new("invocation-1")),
        kind: ActivityKindView::ModelInvocation,
        state: ActivityStateView::Running,
        detail: ActivityDetailView::Model {
            model: "claude-sonnet-4-5".to_string(),
            attempt: 2,
            stream: ModelStreamStateView::Streaming,
        },
        audience: ActivityAudienceView::Operational,
        revision: 7,
        timing: ActivityTimingView {
            total_elapsed_ms: 12_000,
            active_elapsed_ms: 9_000,
            state_elapsed_ms: 3_000,
            started_at_unix_ms: Some(1_000),
            finished_at_unix_ms: None,
        },
    }
}

#[test]
fn activity_view_round_trip_preserves_identity_revision_and_timing() {
    let view = activity_fixture();
    let encoded = serde_json::to_value(&view).expect("encode activity");
    let decoded: ActivityView = serde_json::from_value(encoded).expect("decode activity");

    assert_eq!(decoded, view);
    assert_eq!(decoded.revision, 7);
    assert_eq!(decoded.timing.total_elapsed_ms, 12_000);
    assert_eq!(decoded.timing.active_elapsed_ms, 9_000);
}

#[test]
fn activity_snapshot_carries_complete_views_at_one_run_revision() {
    let snapshot = ActivitySnapshotView {
        run_id: crate::RunId::new("run-1"),
        revision: 9,
        activities: vec![activity_fixture()],
    };

    assert_eq!(snapshot.activities[0].run_id, snapshot.run_id);
    assert_eq!(snapshot.revision, 9);
}

#[test]
fn activity_published_language_serializes_every_closed_variant() {
    let values = [
        serde_json::to_value(ActivityKindView::Run).unwrap(),
        serde_json::to_value(ActivityKindView::RunPhase(
            RunPhaseKindView::PreparingContext,
        ))
        .unwrap(),
        serde_json::to_value(ActivityKindView::ModelInvocation).unwrap(),
        serde_json::to_value(ActivityKindView::ToolCall).unwrap(),
        serde_json::to_value(ActivityKindView::HookDispatch).unwrap(),
        serde_json::to_value(ActivityKindView::Compaction).unwrap(),
        serde_json::to_value(ActivityKindView::Interaction).unwrap(),
        serde_json::to_value(ActivityKindView::SubRun).unwrap(),
        serde_json::to_value(ActivityStateView::Waiting).unwrap(),
        serde_json::to_value(ActivityStateView::Succeeded).unwrap(),
        serde_json::to_value(ActivityStateView::Failed).unwrap(),
        serde_json::to_value(ActivityStateView::Cancelled).unwrap(),
        serde_json::to_value(ActivityStateView::Terminated).unwrap(),
        serde_json::to_value(ActivityAudienceView::User).unwrap(),
        serde_json::to_value(ActivityAudienceView::Diagnostic).unwrap(),
        serde_json::to_value(ActivityDetailView::Run {
            purpose: RunPurposeView::Main,
        })
        .unwrap(),
        serde_json::to_value(ActivityDetailView::Phase {
            phase: RunPhaseKindView::ExecutingTools,
        })
        .unwrap(),
        serde_json::to_value(ActivityDetailView::Tool {
            name: "Read".to_string(),
            summary: Some("src/lib.rs".to_string()),
            parallel_count: 3,
        })
        .unwrap(),
        serde_json::to_value(ActivityDetailView::Hook {
            point: HookPointView::PreToolUse,
            script: "check-policy.sh".to_string(),
            attempt: 1,
        })
        .unwrap(),
        serde_json::to_value(ActivityDetailView::Compact {
            stage: CompactStageView::Mapping,
            work: CompactWorkView::Determinate {
                completed: 2,
                total: 4,
            },
        })
        .unwrap(),
        serde_json::to_value(ActivityDetailView::Interaction {
            kind: InteractionKindView::ToolApproval,
        })
        .unwrap(),
        serde_json::to_value(ActivityDetailView::SubRun {
            role: "reviewer".to_string(),
            model: "claude-sonnet-4-5".to_string(),
        })
        .unwrap(),
    ];

    assert!(values.iter().all(|value| !value.is_null()));
}

#[test]
fn activity_events_keep_change_kind_and_complete_snapshot() {
    let changed = ChatEvent::ActivityChanged {
        kind: ActivityChangeKind::Started,
        activity: activity_fixture(),
    };
    let snapshot = ChatEvent::ActivitySnapshot(ActivitySnapshotView {
        run_id: crate::RunId::new("run-1"),
        revision: 9,
        activities: vec![activity_fixture()],
    });

    assert!(matches!(
        changed,
        ChatEvent::ActivityChanged {
            kind: ActivityChangeKind::Started,
            ..
        }
    ));
    assert!(matches!(
        snapshot,
        ChatEvent::ActivitySnapshot(ActivitySnapshotView { revision: 9, .. })
    ));
}

#[test]
fn wire_document_registers_activity_components() {
    let document = crate::wire::components_document();
    let definitions = document["$defs"].as_object().expect("wire definitions");

    for component in [
        "ActivityId",
        "ActivityView",
        "ActivitySnapshotView",
        "ActivityChangeKind",
    ] {
        assert!(definitions.contains_key(component), "missing {component}");
    }
}
