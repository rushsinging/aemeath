use super::session_change::SessionChange;
use super::session_intent::SessionIntent;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionModel {
    pub current_session_id: Option<String>,
    pub dirty: bool,
    pub message_count: usize,
    pub message_state_revision: u64,
}

impl SessionModel {
    pub fn apply(&mut self, intent: SessionIntent) -> Vec<SessionChange> {
        match intent {
            SessionIntent::SetCurrentSession { id } => {
                self.current_session_id = Some(id.clone());
                vec![SessionChange::CurrentSessionChanged { id }]
            }
            SessionIntent::MessagesSynced { message_count } => {
                self.message_count = message_count;
                self.dirty = false;
                vec![
                    SessionChange::MessagesSynced { message_count },
                    SessionChange::DirtyChanged { dirty: false },
                ]
            }
            SessionIntent::MessageStateChanged {
                message_count,
                revision,
            } => {
                if revision <= self.message_state_revision {
                    return Vec::new();
                }
                let revision_gap = self
                    .message_state_revision
                    .checked_add(1)
                    .filter(|expected| revision > *expected)
                    .map(|expected| revision - expected);
                self.message_count = message_count;
                self.message_state_revision = revision;
                self.dirty = false;
                vec![
                    SessionChange::MessageStateObserved {
                        message_count,
                        revision,
                        revision_gap,
                    },
                    SessionChange::DirtyChanged { dirty: false },
                ]
            }
        }
    }
}

#[cfg(test)]
#[path = "session_model_tests.rs"]
mod tests;
