#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedSubmission {
    pub id: String,
    pub input_id: sdk::InputId,
    pub text: String,
}

impl QueuedSubmission {
    pub fn new(id: impl Into<String>, input_id: sdk::InputId, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            input_id,
            text: text.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queued_submission_stores_text() {
        let input_id = sdk::InputId::new_v7();
        let queued = QueuedSubmission::new("q1", input_id, "hello");
        assert_eq!(queued.text, "hello");
    }

    #[test]
    fn test_queued_submission_allows_empty_text() {
        let input_id = sdk::InputId::new_v7();
        let queued = QueuedSubmission::new("q1", input_id, "");
        assert_eq!(queued.text, "");
    }

    #[test]
    fn test_queued_submission_preserves_id() {
        let input_id = sdk::InputId::new_v7();
        let queued = QueuedSubmission::new("q1", input_id, "hello");
        assert_eq!(queued.id, "q1");
    }

    #[test]
    fn test_queued_submission_preserves_input_id() {
        let input_id = sdk::InputId::new_v7();
        let cloned = input_id.clone();
        let queued = QueuedSubmission::new("q1", input_id, "hello");
        assert_eq!(queued.input_id, cloned);
    }
}
