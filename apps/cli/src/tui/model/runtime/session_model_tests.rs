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
    model.apply(SessionIntent::MessagesSynced { message_count: 3 });
    assert!(!model.dirty);
    assert_eq!(model.message_count, 3);
}
