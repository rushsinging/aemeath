use super::change::{SessionChange, SessionSaveStatus};
use super::intent::SessionIntent;
use super::resume::SessionResumeCandidate;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionModel {
    pub current_session_id: Option<String>,
    pub dirty: bool,
    pub message_count: usize,
    pub resume_candidates: Vec<SessionResumeCandidate>,
    pub save_status: SessionSaveStatus,
}

impl SessionModel {
    pub fn apply(&mut self, intent: SessionIntent) -> Vec<SessionChange> {
        match intent {
            SessionIntent::SetCurrentSession { id } => {
                self.current_session_id = Some(id.clone());
                vec![SessionChange::CurrentSessionChanged { id }]
            }
            SessionIntent::MarkDirty => {
                self.dirty = true;
                vec![SessionChange::DirtyChanged { dirty: true }]
            }
            SessionIntent::MessagesSynced { message_count } => {
                self.message_count = message_count;
                self.dirty = false;
                vec![
                    SessionChange::MessagesSynced { message_count },
                    SessionChange::DirtyChanged { dirty: false },
                ]
            }
            SessionIntent::SaveStarted => {
                self.save_status = SessionSaveStatus::Saving;
                vec![SessionChange::SaveStatusChanged {
                    status: self.save_status.clone(),
                }]
            }
            SessionIntent::SaveFinished => {
                self.save_status = SessionSaveStatus::Saved;
                self.dirty = false;
                vec![
                    SessionChange::SaveStatusChanged {
                        status: self.save_status.clone(),
                    },
                    SessionChange::DirtyChanged { dirty: false },
                ]
            }
            SessionIntent::SaveFailed { message } => {
                self.save_status = SessionSaveStatus::Failed { message };
                vec![SessionChange::SaveStatusChanged {
                    status: self.save_status.clone(),
                }]
            }
            SessionIntent::ResumeCandidatesLoaded { candidates } => {
                self.resume_candidates = candidates.clone();
                vec![SessionChange::ResumeCandidatesChanged { candidates }]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_model_sets_current_session() {
        let mut model = SessionModel::default();
        model.apply(SessionIntent::SetCurrentSession { id: "s1".into() });
        assert_eq!(model.current_session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn test_session_model_sync_clears_dirty() {
        let mut model = SessionModel::default();
        model.apply(SessionIntent::MarkDirty);
        model.apply(SessionIntent::MessagesSynced { message_count: 3 });
        assert!(!model.dirty);
        assert_eq!(model.message_count, 3);
    }

    #[test]
    fn test_session_model_save_failed_records_status() {
        let mut model = SessionModel::default();
        model.apply(SessionIntent::SaveFailed {
            message: "磁盘错误".into(),
        });
        assert!(matches!(
            model.save_status,
            SessionSaveStatus::Failed { ref message } if message == "磁盘错误"
        ));
    }
}
