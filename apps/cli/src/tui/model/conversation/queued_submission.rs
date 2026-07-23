#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedSubmission {
    pub id: String,
    pub input_id: String,
    pub text: String,
}

impl QueuedSubmission {
    pub fn new(
        id: impl Into<String>,
        input_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            input_id: input_id.into(),
            text: text.into(),
        }
    }
}

#[cfg(test)]
mod tests {
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
}
