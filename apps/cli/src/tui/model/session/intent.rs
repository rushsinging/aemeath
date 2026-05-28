#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionIntent {
    SetCurrentSession {
        id: String,
    },
    MarkDirty,
    MessagesSynced {
        message_count: usize,
    },
    SaveStarted,
    SaveFinished,
    SaveFailed {
        message: String,
    },
    ResumeCandidatesLoaded {
        candidates: Vec<SessionResumeCandidate>,
    },
}

use super::resume::SessionResumeCandidate;
