use super::intent::{ObserveActivityChange, ReplaceActivitySnapshot};
use super::interaction::UiRunId;
use super::model::ConversationModel;
use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityChangeKind, TuiActivityDetail, TuiActivityKind,
    TuiActivityObservation, TuiActivitySnapshot, TuiActivitySource, TuiActivityState,
    TuiActivityTiming, TuiRunPurpose, UiActivityId,
};

fn activity(run_id: &str, activity_id: &str, revision: u64) -> TuiActivityObservation {
    TuiActivityObservation {
        id: UiActivityId::from(activity_id),
        run_id: UiRunId::from(run_id),
        run_step_id: None,
        parent_activity_id: None,
        source: TuiActivitySource::Run,
        kind: TuiActivityKind::Run,
        state: TuiActivityState::Running,
        detail: TuiActivityDetail::Run {
            purpose: TuiRunPurpose::Main,
        },
        audience: TuiActivityAudience::User,
        revision,
        timing: TuiActivityTiming::default(),
    }
}

#[test]
fn snapshot_heartbeat_refreshes_timing_at_same_business_revision() {
    let mut model = ConversationModel::default();
    let mut root = activity("run-1", "root", 1);
    root.timing.total_elapsed_ms = 1_000;
    model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 1,
            heartbeat_sequence: 0,
            activities: vec![root.clone()],
        },
    });
    root.timing.total_elapsed_ms = 2_000;

    let changes = model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 1,
            heartbeat_sequence: 1,
            activities: vec![root],
        },
    });

    assert_eq!(changes.len(), 1);
    assert_eq!(
        model
            .activity_observations()
            .activity(&UiActivityId::from("root"))
            .unwrap()
            .timing
            .total_elapsed_ms,
        2_000
    );
}

#[test]
fn duplicate_or_stale_snapshot_heartbeat_is_rejected() {
    let mut model = ConversationModel::default();
    let root = activity("run-1", "root", 1);
    model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 1,
            heartbeat_sequence: 2,
            activities: vec![root.clone()],
        },
    });

    for heartbeat_sequence in [2, 1] {
        let changes = model.apply(ReplaceActivitySnapshot {
            snapshot: TuiActivitySnapshot {
                run_id: UiRunId::from("run-1"),
                revision: 1,
                heartbeat_sequence,
                activities: vec![root.clone()],
            },
        });
        assert!(changes.is_empty());
    }
}

#[test]
fn first_increment_above_revision_one_marks_run_stale() {
    let mut model = ConversationModel::default();

    let changes = model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Started,
        activity: activity("run-1", "activity-2", 2),
    });

    assert_eq!(changes.len(), 1);
    assert!(model.activity_observations().activities().is_empty());
    assert!(model
        .activity_observations()
        .is_stale(&UiRunId::from("run-1")));
    assert_eq!(
        model
            .activity_observations()
            .revision_for(&UiRunId::from("run-1")),
        None
    );
}

#[test]
fn snapshot_with_same_revision_repairs_stale_run() {
    let mut model = ConversationModel::default();
    model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Started,
        activity: activity("run-1", "activity-1", 1),
    });
    model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Updated,
        activity: activity("run-1", "activity-3", 3),
    });

    let changes = model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 1,
            heartbeat_sequence: 0,
            activities: vec![activity("run-1", "snapshot-activity", 1)],
        },
    });

    assert_eq!(changes.len(), 1);
    assert!(!model
        .activity_observations()
        .is_stale(&UiRunId::from("run-1")));
    assert_eq!(
        model.activity_observations().activities()[0].id.as_str(),
        "snapshot-activity"
    );
}

#[test]
fn increment_upserts_by_identity_and_ignores_stale_or_duplicate_revision() {
    let mut model = ConversationModel::default();
    model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Started,
        activity: activity("run-1", "activity-1", 1),
    });
    let duplicate_changes = model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Updated,
        activity: activity("run-1", "activity-1", 1),
    });
    let stale_changes = model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Updated,
        activity: activity("run-1", "activity-1", 0),
    });

    assert!(duplicate_changes.is_empty());
    assert!(stale_changes.is_empty());
    assert_eq!(model.activity_observations().activities().len(), 1);
    assert_eq!(
        model
            .activity_observations()
            .revision_for(&UiRunId::from("run-1")),
        Some(1)
    );
}

#[test]
fn contiguous_increment_advances_revision_and_replaces_complete_fact() {
    let mut model = ConversationModel::default();
    model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Started,
        activity: activity("run-1", "activity-1", 1),
    });
    let mut updated = activity("run-1", "activity-1", 2);
    updated.state = TuiActivityState::Succeeded;
    updated.timing.total_elapsed_ms = 1_500;

    let changes = model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Finished,
        activity: updated,
    });

    assert_eq!(changes.len(), 1);
    let stored = model
        .activity_observations()
        .activities()
        .first()
        .expect("activity observation");
    assert_eq!(stored.revision, 2);
    assert_eq!(stored.state, TuiActivityState::Succeeded);
    assert_eq!(stored.timing.total_elapsed_ms, 1_500);
    assert_eq!(
        model
            .activity_observations()
            .revision_for(&UiRunId::from("run-1")),
        Some(2)
    );
    assert!(!model
        .activity_observations()
        .is_stale(&UiRunId::from("run-1")));
}

#[test]
fn revision_gap_marks_run_stale_without_applying_ambiguous_increment() {
    let mut model = ConversationModel::default();
    model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Started,
        activity: activity("run-1", "activity-1", 1),
    });
    let mut gapped = activity("run-1", "activity-1", 3);
    gapped.state = TuiActivityState::Failed;

    let changes = model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Finished,
        activity: gapped,
    });

    assert_eq!(changes.len(), 1);
    let stored = model
        .activity_observations()
        .activities()
        .first()
        .expect("activity observation");
    assert_eq!(stored.revision, 1);
    assert_eq!(stored.state, TuiActivityState::Running);
    assert!(model
        .activity_observations()
        .is_stale(&UiRunId::from("run-1")));
    assert_eq!(
        model
            .activity_observations()
            .revision_for(&UiRunId::from("run-1")),
        Some(1)
    );
}

#[test]
fn snapshot_atomically_replaces_one_run_and_clears_stale_marker() {
    let mut model = ConversationModel::default();
    model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Started,
        activity: activity("run-1", "old-activity", 1),
    });
    model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Started,
        activity: activity("run-2", "other-activity", 1),
    });
    model.apply(ObserveActivityChange {
        kind: TuiActivityChangeKind::Updated,
        activity: activity("run-1", "old-activity", 3),
    });

    let changes = model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 4,
            heartbeat_sequence: 0,
            activities: vec![activity("run-1", "new-activity", 4)],
        },
    });

    assert_eq!(changes.len(), 1);
    let observations = model.activity_observations();
    assert_eq!(observations.activities().len(), 2);
    assert!(observations
        .activities()
        .iter()
        .any(|item| item.id.as_str() == "new-activity"));
    assert!(observations
        .activities()
        .iter()
        .any(|item| item.id.as_str() == "other-activity"));
    assert!(observations
        .activities()
        .iter()
        .all(|item| item.id.as_str() != "old-activity"));
    assert_eq!(observations.revision_for(&UiRunId::from("run-1")), Some(4));
    assert!(!observations.is_stale(&UiRunId::from("run-1")));
}

#[test]
fn older_snapshot_cannot_roll_back_newer_run_mirror() {
    let mut model = ConversationModel::default();
    model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 5,
            heartbeat_sequence: 0,
            activities: vec![activity("run-1", "activity-5", 5)],
        },
    });

    let changes = model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 4,
            heartbeat_sequence: 0,
            activities: vec![activity("run-1", "activity-4", 4)],
        },
    });

    assert!(changes.is_empty());
    let observations = model.activity_observations();
    assert_eq!(observations.revision_for(&UiRunId::from("run-1")), Some(5));
    assert_eq!(observations.activities()[0].id.as_str(), "activity-5");
}

#[test]
fn snapshot_rejects_activity_revision_newer_than_snapshot_revision() {
    let mut model = ConversationModel::default();

    let changes = model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 2,
            heartbeat_sequence: 0,
            activities: vec![activity("run-1", "future-activity", 3)],
        },
    });

    assert!(changes.is_empty());
    assert!(model.activity_observations().activities().is_empty());
}

#[test]
fn snapshot_rejects_foreign_run_activity_without_mutating_model() {
    let mut model = ConversationModel::default();

    let changes = model.apply(ReplaceActivitySnapshot {
        snapshot: TuiActivitySnapshot {
            run_id: UiRunId::from("run-1"),
            revision: 1,
            heartbeat_sequence: 0,
            activities: vec![activity("run-2", "foreign", 1)],
        },
    });

    assert!(changes.is_empty());
    assert!(model.activity_observations().activities().is_empty());
    assert_eq!(
        model
            .activity_observations()
            .revision_for(&UiRunId::from("run-1")),
        None
    );
}
