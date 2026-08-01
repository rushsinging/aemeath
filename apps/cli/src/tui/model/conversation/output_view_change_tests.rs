use super::super::intent::{AppendUserMessage, AssistantText};
use super::{ConversationModel, OutputViewChange, OutputViewChanges, OUTPUT_VIEW_JOURNAL_CAPACITY};
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
use crate::tui::model::output_timeline::OutputTimelineItem;

#[test]
fn append_does_not_read_existing_timeline_identities() {
    let mut model = ConversationModel::default();
    for index in 0..5_000 {
        model.timeline.push(OutputTimelineItem::UserMessage {
            id: format!("history-{index}"),
            text: String::new(),
        });
    }
    model.timeline.reset_identity_read_count();

    model.apply(AppendUserMessage {
        text: "incremental".to_string(),
    });

    assert_eq!(model.timeline.identity_read_count(), 0);
}

#[test]
fn append_and_streaming_update_publish_payload_free_output_view_changes() {
    let mut model = ConversationModel::default();
    let cursor = model.output_view_cursor();

    model.apply(AppendUserMessage {
        text: "secret-user-payload".to_string(),
    });
    let (next_cursor, changes) = match model.output_view_changes_since(cursor) {
        OutputViewChanges::Delta {
            next_cursor,
            changes,
        } => (next_cursor, changes),
        OutputViewChanges::RebuildRequired { .. } => panic!("fresh cursor must receive delta"),
    };
    assert_eq!(
        changes,
        vec![OutputViewChange::Append {
            item_id: "user-1".to_string(),
        }]
    );
    assert!(!format!("{changes:?}").contains("secret-user-payload"));

    model.apply(AssistantText {
        chat_id: ChatId::new("chat-1"),
        turn_id: ChatTurnId::new("turn-1"),
        text: "first-secret-chunk".to_string(),
    });
    let (stream_cursor, first_stream_changes) = match model.output_view_changes_since(next_cursor) {
        OutputViewChanges::Delta {
            next_cursor,
            changes,
        } => (next_cursor, changes),
        OutputViewChanges::RebuildRequired { .. } => panic!("fresh cursor must receive delta"),
    };
    assert_eq!(
        first_stream_changes,
        vec![OutputViewChange::Append {
            item_id: "assistant-2".to_string(),
        }]
    );

    model.apply(AssistantText {
        chat_id: ChatId::new("chat-1"),
        turn_id: ChatTurnId::new("turn-1"),
        text: "second-secret-chunk".to_string(),
    });
    let changes = match model.output_view_changes_since(stream_cursor) {
        OutputViewChanges::Delta { changes, .. } => changes,
        OutputViewChanges::RebuildRequired { .. } => panic!("fresh cursor must receive delta"),
    };
    assert_eq!(
        changes,
        vec![OutputViewChange::Update {
            item_id: "assistant-2".to_string(),
        }]
    );
    let debug = format!("{changes:?}");
    assert!(!debug.contains("first-secret-chunk"));
    assert!(!debug.contains("second-secret-chunk"));
}

#[test]
fn expired_output_view_cursor_requires_rebuild() {
    let mut model = ConversationModel::default();
    let stale = model.output_view_cursor();
    for index in 0..=OUTPUT_VIEW_JOURNAL_CAPACITY {
        model.apply(AppendUserMessage {
            text: format!("message-{index}"),
        });
    }

    assert!(matches!(
        model.output_view_changes_since(stale),
        OutputViewChanges::RebuildRequired { .. }
    ));
}
