use super::*;

fn state(
    session_id: &str,
    revision: u64,
    subject: &str,
) -> crate::tui::adapter::runtime_view::TuiTaskState {
    use crate::tui::adapter::runtime_view::*;
    TuiTaskState {
        session_id: session_id.to_owned(),
        revision,
        current_batch: Some(TuiTaskBatch {
            id: 1,
            summary: Some("batch".to_owned()),
            status: TuiTaskBatchStatus::Active,
        }),
        total: 1,
        completed: 0,
        in_progress: 0,
        items: vec![TuiTaskItem {
            id: 1,
            sequence: 1,
            subject: subject.to_owned(),
            status: TuiTaskItemStatus::Pending,
            priority: TuiTaskPriority::Normal,
            blocked_by_sequences: Vec::new(),
        }],
        hidden_count: 0,
    }
}

#[test]
fn replacement_is_session_scoped_and_revision_ordered() {
    let mut snapshot = TaskStatusSnapshot::default();
    assert!(snapshot.replace(state("session-a", 42, "new")));
    assert!(!snapshot.replace(state("session-a", 41, "stale")));
    assert_eq!(snapshot.lines[1], "□ #1 new");
    assert!(snapshot.replace(state("session-b", 1, "other")));
    assert_eq!(snapshot.lines[1], "□ #1 other");
}
