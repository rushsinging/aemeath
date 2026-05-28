#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedSubmission {
    pub id: String,
    pub text: String,
}

impl QueuedSubmission {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queued_submission_stores_text() {
        let queued = QueuedSubmission::new("q1", "hello");
        assert_eq!(queued.text, "hello");
    }

    #[test]
    fn test_queued_submission_allows_empty_text() {
        let queued = QueuedSubmission::new("q1", "");
        assert_eq!(queued.text, "");
    }

    #[test]
    fn test_queued_submission_preserves_id() {
        let queued = QueuedSubmission::new("q1", "hello");
        assert_eq!(queued.id, "q1");
    }
}
