use super::session_resume::SessionResumeCandidate;

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
    MessageStateObserved {
        message_count: usize,
        revision: u64,
        revision_gap: Option<u64>,
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
