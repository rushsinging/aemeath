#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionResumeCandidate {
    pub id: String,
    pub title: String,
}

impl SessionResumeCandidate {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}
