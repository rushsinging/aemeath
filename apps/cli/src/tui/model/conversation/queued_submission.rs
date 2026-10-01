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
#[path = "queued_submission_tests.rs"]
mod tests;
