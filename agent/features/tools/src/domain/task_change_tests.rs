use super::task_change::{CommittedTaskChange, TaskChangeFact};
use task::{BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPriority, TaskStatus, TaskStore};

fn task_spec(subject: &str) -> TaskCreateSpec {
    TaskCreateSpec::try_new(
        subject.to_owned(),
        "description".to_owned(),
        None,
        TaskPriority::Normal,
    )
    .expect("task fixture must be valid")
}

#[test]
fn committed_create_exposes_revision_and_created_fact() {
    let store = TaskStore::new();
    let batch = store
        .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let created = store.create_task(task_spec("first"), 2).unwrap();

    let change = CommittedTaskChange::from_command_result(&created).expect("create commits");

    assert_eq!(change.revision(), created.revision().unwrap());
    assert_eq!(change.revision(), store.revision());
    assert!(matches!(
        change.facts(),
        [TaskChangeFact::Created { task_id }] if *task_id == created.value.id()
    ));
    assert_eq!(created.value.batch(), batch.value.id());
}

#[test]
fn committed_completion_exposes_completed_fact_only_on_real_transition() {
    let store = TaskStore::new();
    store
        .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let created = store.create_task(task_spec("first"), 2).unwrap().value;
    store
        .transition(created.id(), TaskStatus::InProgress, 3)
        .unwrap();
    let completed = store
        .transition(created.id(), TaskStatus::Completed, 4)
        .unwrap();

    let change = CommittedTaskChange::from_command_result(&completed).expect("completion commits");

    assert_eq!(change.revision(), completed.revision().unwrap());
    assert!(matches!(
        change.facts(),
        [TaskChangeFact::Completed { task_id }] if *task_id == created.id()
    ));
}

#[test]
fn no_op_command_has_no_committed_task_change() {
    let store = TaskStore::new();
    store
        .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let created = store.create_task(task_spec("first"), 2).unwrap().value;
    let no_op = store
        .set_subject(created.id(), created.subject().to_owned(), 3)
        .unwrap();

    assert!(no_op.revision().is_none());
    assert!(CommittedTaskChange::from_command_result(&no_op).is_none());
}

#[test]
fn non_hook_mutation_keeps_commit_revision_without_hook_fact() {
    let store = TaskStore::new();
    store
        .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let created = store.create_task(task_spec("first"), 2).unwrap().value;
    let changed = store
        .set_priority(created.id(), TaskPriority::High, 3)
        .unwrap();

    let change = CommittedTaskChange::from_command_result(&changed).expect("priority commits");

    assert!(change.facts().is_empty());
    assert_eq!(change.revision(), changed.revision().unwrap());
}
