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
mod tests {
    use super::*;

    #[test]
    fn test_session_model_sets_current_session() {
        let mut model = SessionModel::default();
        model.apply(SessionIntent::SetCurrentSession { id: "s1".into() });
        assert_eq!(model.current_session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn ignores_stale_or_duplicate_message_state_revision() {
        let mut model = SessionModel::default();
        model.apply(SessionIntent::MessageStateChanged {
            message_count: 4,
            revision: 2,
        });
        model.apply(SessionIntent::MessageStateChanged {
            message_count: 1,
            revision: 1,
        });
        model.apply(SessionIntent::MessageStateChanged {
            message_count: 9,
            revision: 2,
        });

        assert_eq!(model.message_count, 4);
        assert_eq!(model.message_state_revision, 2);
    }

    #[test]
    fn reports_revision_gap_while_accepting_newer_projection() {
        let mut model = SessionModel::default();
        let changes = model.apply(SessionIntent::MessageStateChanged {
            message_count: 4,
            revision: 3,
        });

        assert!(matches!(
            changes.as_slice(),
            [
                SessionChange::MessageStateObserved {
                    message_count: 4,
                    revision: 3,
                    revision_gap: Some(2),
                },
                SessionChange::DirtyChanged { dirty: false },
            ]
        ));
    }

    #[test]
    fn test_session_model_sync_clears_dirty() {
        let mut model = SessionModel::default();
        model.apply(SessionIntent::MarkDirty);
        model.apply(SessionIntent::MessagesSynced { message_count: 3 });
        assert!(!model.dirty);
        assert_eq!(model.message_count, 3);
    }
}
