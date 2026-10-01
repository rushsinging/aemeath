use super::*;
use crate::tui::model::conversation::intent::*;

#[test]
fn test_seed_banner_pushes_system_blocks() {
    let mut model = ConversationModel::default();
    let changes = model.seed_banner();
    assert_eq!(
        model
            .timeline
            .items()
            .iter()
            .filter(|b| matches!(b, OutputTimelineItem::System { .. }))
            .count(),
        BANNER_LINES.len()
    );
    assert!(changes
        .iter()
        .any(|c| matches!(c, ConversationChange::SystemMessageAppended { .. })));
}

#[test]
fn test_seed_banner_first_block_is_title() {
    let mut model = ConversationModel::default();
    model.seed_banner();
    let first = model.timeline.items().first().expect("banner block");
    assert!(matches!(
        first,
        OutputTimelineItem::System { text, .. } if text == "Aemeath - AI Agent"
    ));
}

#[test]
fn test_append_system_message_resets_active_text_block() {
    let mut model = ConversationModel::default();
    model.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    model.apply(AppendUserMessage {
        text: "hi".to_string(),
    });
    model.apply(AssistantText {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        text: "streaming".to_string(),
    });
    model.apply(AppendSystemMessage {
        text: "notice".to_string(),
    });
    model.apply(AssistantText {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        text: "after".to_string(),
    });
    let assistant_blocks = model
        .timeline
        .items()
        .iter()
        .filter(|b| matches!(b, OutputTimelineItem::AssistantText { .. }))
        .count();
    assert_eq!(assistant_blocks, 2);
}

#[test]
fn test_append_error_pushes_error_block() {
    let mut model = ConversationModel::default();
    let changes = model.append_error("坏了".to_string());
    assert!(matches!(
        model.timeline.items().last(),
        Some(OutputTimelineItem::Error { text, .. }) if text == "坏了"
    ));
    assert!(changes
        .iter()
        .any(|c| matches!(c, ConversationChange::ErrorAppended { .. })));
}
