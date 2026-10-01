use super::*;

#[test]
fn test_queued_submission_stores_text() {
    let queued = QueuedSubmission::new("q1", "input-1", "hello");
    assert_eq!(queued.text, "hello");
}

#[test]
fn test_queued_submission_allows_empty_text() {
    let queued = QueuedSubmission::new("q1", "input-1", "");
    assert_eq!(queued.text, "");
}

#[test]
fn test_queued_submission_preserves_id() {
    let queued = QueuedSubmission::new("q1", "input-1", "hello");
    assert_eq!(queued.id, "q1");
}

#[test]
fn test_queued_submission_preserves_input_id() {
    let queued = QueuedSubmission::new("q1", "input-1", "hello");
    assert_eq!(queued.input_id, "input-1");
}
