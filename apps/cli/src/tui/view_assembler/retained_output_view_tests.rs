use super::RetainedOutputView;
use crate::tui::app::event::{ModelStreamWaitingView, UiTurnContext};
use crate::tui::model::conversation::block::AskUserSlot;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
use crate::tui::model::conversation::intent::{
    AppendUserMessage, AssistantText, DismissAskUserBatch, ShowAskUserBatch,
    UpsertModelStreamPlaceholder,
};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::view_assembler::output::OutputViewAssembler;
use std::sync::Arc;

fn root_ids(view: &RetainedOutputView) -> Vec<&str> {
    view.roots()
        .iter()
        .map(|root| root.block_id.as_str())
        .collect()
}

#[test]
fn append_reuses_existing_roots_and_creates_only_the_new_root() {
    let mut model = ConversationModel::default();
    for index in 0..128 {
        model.apply(AppendUserMessage {
            text: format!("message-{index}"),
        });
    }
    let mut view = RetainedOutputView::default();
    let initial = view.sync(&model, None);
    let retained = view.roots().to_vec();
    assert_eq!(initial.rebuilt_roots, 128);

    model.apply(AppendUserMessage {
        text: "appended".to_string(),
    });
    let update = view.sync(&model, None);

    assert_eq!(update.created_roots, 1);
    assert_eq!(update.touched_roots, 1);
    assert_eq!(update.reused_roots, 128);
    assert_eq!(view.roots().len(), 129);
    assert!(retained
        .iter()
        .zip(view.roots())
        .all(|(before, after)| Arc::ptr_eq(before, after)));
}

#[test]
fn streaming_update_replaces_only_the_changed_root() {
    let mut model = ConversationModel::default();
    model.apply(AppendUserMessage {
        text: "stable".to_string(),
    });
    model.apply(AssistantText {
        chat_id: ChatId::new("chat-1"),
        turn_id: ChatTurnId::new("turn-1"),
        text: "first".to_string(),
    });
    let mut view = RetainedOutputView::default();
    view.sync(&model, None);
    let stable = Arc::clone(&view.roots()[0]);
    let streaming = Arc::clone(&view.roots()[1]);

    model.apply(AssistantText {
        chat_id: ChatId::new("chat-1"),
        turn_id: ChatTurnId::new("turn-1"),
        text: " second".to_string(),
    });
    let update = view.sync(&model, None);

    assert_eq!(update.created_roots, 1);
    assert_eq!(update.touched_roots, 1);
    assert_eq!(update.reused_roots, 1);
    assert!(Arc::ptr_eq(&stable, &view.roots()[0]));
    assert!(!Arc::ptr_eq(&streaming, &view.roots()[1]));
}

#[test]
fn reset_and_placeholder_changes_match_full_assembly() {
    let mut model = ConversationModel::default();
    model.apply(AppendUserMessage {
        text: "before-reset".to_string(),
    });
    let mut view = RetainedOutputView::default();
    view.sync(&model, None);

    model.reset();
    model.apply(AppendUserMessage {
        text: "after-reset".to_string(),
    });
    model.apply(UpsertModelStreamPlaceholder {
        placeholder: ModelStreamWaitingView {
            context: UiTurnContext {
                chat_id: ChatId::new("chat-1"),
                turn_id: ChatTurnId::new("turn-1"),
            },
            elapsed_secs: 3,
            phase: "waiting".to_string(),
        },
    });
    let update = view.sync(&model, None);
    let reference = OutputViewAssembler::assemble_from_conversation(&model, model.revision(), None);

    assert!(update.did_rebuild);
    assert_eq!(root_ids(&view), vec!["user-1", "model-stream-placeholder"]);
    assert_eq!(view.roots(), reference.roots.as_slice());
}

#[test]
fn dismiss_removes_only_the_active_ask_user_root() {
    let mut model = ConversationModel::default();
    model.apply(AppendUserMessage {
        text: "stable".to_string(),
    });
    model.apply(ShowAskUserBatch {
        slots: vec![AskUserSlot {
            id: "question-1".to_string(),
            question_seq: 0,
            question: "Continue?".to_string(),
            options: vec![sdk::OptionItem::title_only("Yes".to_string())],
            llm_option_count: 1,
            multi_select: false,
            default: None,
            answer: None,
        }],
    });
    let mut view = RetainedOutputView::default();
    view.sync(&model, None);
    let stable = Arc::clone(&view.roots()[0]);

    model.apply(DismissAskUserBatch);
    let update = view.sync(&model, None);

    assert!(!update.did_rebuild);
    assert_eq!(update.touched_roots, 1);
    assert_eq!(view.roots().len(), 1);
    assert!(Arc::ptr_eq(&stable, &view.roots()[0]));
}

#[test]
fn workspace_change_rebuilds_tool_sensitive_roots_once() {
    let mut model = ConversationModel::default();
    model.apply(AppendUserMessage {
        text: "stable".to_string(),
    });
    let mut view = RetainedOutputView::default();
    view.sync(&model, Some(std::path::Path::new("/tmp/a")));
    let first = Arc::clone(&view.roots()[0]);

    let update = view.sync(&model, Some(std::path::Path::new("/tmp/b")));

    assert!(update.did_rebuild);
    assert!(!Arc::ptr_eq(&first, &view.roots()[0]));
    assert_eq!(update.rebuilt_roots, 1);
}
