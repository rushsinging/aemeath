use super::resume::SessionResumeCandidate;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionChange {
    CurrentSessionChanged {
        id: String,
    },
    DirtyChanged {
        dirty: bool,
    },
    MessagesSynced {
        message_count: usize,
    },
    SaveStatusChanged {
        status: SessionSaveStatus,
    },
    ResumeCandidatesChanged {
        candidates: Vec<SessionResumeCandidate>,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum SessionSaveStatus {
    #[default]
    Idle,
    Saving,
    Saved,
    Failed {
        message: String,
    },
}
