use task::{
    wire_task, BatchCreateSpec, TaskCreateSpec, TaskPriority, TaskSnapshot,
    TaskSnapshotValidationError,
};

fn task_spec(subject: &str) -> TaskCreateSpec {
    TaskCreateSpec::try_new(
        subject.to_owned(),
        String::new(),
        None,
        TaskPriority::Normal,
    )
    .unwrap()
}

#[test]
fn task_persist_round_trips_nonempty_snapshot_between_capability_views() {
    let source = wire_task();
    let source_access = source.access();
    let source_persist = source.persist();
    source_access
        .create_batch(BatchCreateSpec::try_new("source batch".into()).unwrap(), 1)
        .unwrap();
    let created = source_access
        .create_task(task_spec("source task"), 2)
        .unwrap()
        .value;

    let snapshot = source_persist.collect_snapshot();
    let target = wire_task();
    let target_access = target.access();
    let target_persist = target.persist();
    let prepared = target_persist
        .prepare_restore(&snapshot)
        .expect("captured snapshot must restore through the public port");
    target_persist.commit_restore(prepared);

    assert_eq!(target_access.get(created.id()), Some(created));
    assert_eq!(target_persist.collect_snapshot(), snapshot);
}

#[test]
fn task_persist_rejected_snapshot_keeps_live_view_unchanged() {
    let wiring = wire_task();
    let access = wiring.access();
    let persist = wiring.persist();
    access
        .create_batch(BatchCreateSpec::try_new("live batch".into()).unwrap(), 1)
        .unwrap();
    access.create_task(task_spec("live task"), 2).unwrap();
    let before = persist.collect_snapshot();
    let invalid = TaskSnapshot::decode(
        br#"{"schema_version":2,"revision":"1","tasks":[{"id":"1","batch":"1","subject":"t","description":"","active_form":null,"session_id":null,"tags":[],"blocked_by":["1"],"status":"pending","priority":"normal","created_at":1,"updated_at":1,"started_at":null,"completed_at":null}],"next_task_id":"2","next_batch_id":"2","current_batch":"1","batches":[{"id":"1","summary":"b","status":"active","created_at":1,"last_active_turn":0,"silence_turns":0}]}"#,
    )
    .expect("fixture must decode");

    assert!(matches!(
        persist.prepare_restore(&invalid),
        Err(TaskSnapshotValidationError::SelfDependency { .. })
    ));
    assert_eq!(persist.collect_snapshot(), before);
}
