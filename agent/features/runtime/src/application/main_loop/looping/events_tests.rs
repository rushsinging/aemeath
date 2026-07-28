use super::events::is_task_store_mutation;

#[test]
fn task_store_mutation_classification_includes_task_block_by() {
    for name in [
        "TaskCreate",
        "TaskUpdate",
        "TaskBlockBy",
        "TaskStop",
        "TaskListCreate",
        "TaskListComplete",
    ] {
        assert!(is_task_store_mutation(name), "{name}");
    }
    assert!(!is_task_store_mutation("TaskListGet"));
}
