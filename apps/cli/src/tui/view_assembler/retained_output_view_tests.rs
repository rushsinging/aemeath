use super::RetainedOutputView;
use crate::tui::app::event::{ModelStreamWaitingView, UiTurnContext};
use crate::tui::model::conversation::block::AskUserSlot;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
use crate::tui::model::conversation::intent::{
    AppendUserMessage, AssistantText, DismissAskUserBatch, ShowAskUserBatch,
    UpsertModelStreamPlaceholder,
};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::render::output::document_renderer::OutputRenderWindow;
use std::sync::Arc;

fn full_window() -> OutputRenderWindow {
    OutputRenderWindow {
        line_limit: usize::MAX,
        tail_offset: 0,
    }
}

fn materialize_all<'a>(
    view: &'a mut RetainedOutputView,
    model: &ConversationModel,
    workspace_root: Option<&std::path::Path>,
) -> super::MaterializedOutputWindow {
    view.materialize_window(model, workspace_root, full_window())
}

fn root_ids(view: &RetainedOutputView) -> Vec<&str> {
    view.roots()
        .iter()
        .map(|root| root.block_id.as_str())
        .collect()
}

#[test]
fn cold_window_materializes_only_tail_candidates() {
    let mut model = ConversationModel::default();
    for index in 0..100_000 {
        model.apply(AppendUserMessage {
            text: format!("message-{index}"),
        });
    }
    let mut view = RetainedOutputView::default();

    let window = view.materialize_window(
        &model,
        None,
        OutputRenderWindow {
            line_limit: 20,
            tail_offset: 0,
        },
    );

    assert_eq!(window.indexed_items, 100_000);
    assert!(window.view_model.roots.len() <= 20);
    assert!(window.view_model.roots.len() < window.indexed_items);
    assert!(view.roots().len() <= 20);
    assert_eq!(
        window
            .view_model
            .roots
            .last()
            .map(|root| root.block_id.as_str()),
        Some("user-100000")
    );
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
    let initial = materialize_all(&mut view, &model, None);
    let retained = view.roots().to_vec();
    assert_eq!(initial.stats.rebuilt_roots, 128);

    model.apply(AppendUserMessage {
        text: "appended".to_string(),
    });
    let update = materialize_all(&mut view, &model, None);

    assert_eq!(update.stats.created_roots, 1);
    assert_eq!(update.stats.touched_roots, 1);
    assert_eq!(update.stats.reused_roots, 128);
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
    materialize_all(&mut view, &model, None);
    let stable = Arc::clone(&view.roots()[0]);
    let streaming = Arc::clone(&view.roots()[1]);

    model.apply(AssistantText {
        chat_id: ChatId::new("chat-1"),
        turn_id: ChatTurnId::new("turn-1"),
        text: " second".to_string(),
    });
    let update = materialize_all(&mut view, &model, None);

    assert_eq!(update.stats.created_roots, 1);
    assert_eq!(update.stats.touched_roots, 1);
    assert_eq!(update.stats.reused_roots, 1);
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
    materialize_all(&mut view, &model, None);

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
    let update = materialize_all(&mut view, &model, None);
    assert!(update.stats.did_rebuild);
    assert_eq!(root_ids(&view), vec!["user-1", "model-stream-placeholder"]);
    assert_eq!(update.view_model.roots, view.roots());
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
    materialize_all(&mut view, &model, None);
    let stable = Arc::clone(&view.roots()[0]);

    model.apply(DismissAskUserBatch);
    let update = materialize_all(&mut view, &model, None);

    assert!(!update.stats.did_rebuild);
    assert_eq!(update.stats.touched_roots, 0);
    assert_eq!(update.stats.reused_roots, 1);
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
    materialize_all(&mut view, &model, Some(std::path::Path::new("/tmp/a")));
    let first = Arc::clone(&view.roots()[0]);

    let update = materialize_all(&mut view, &model, Some(std::path::Path::new("/tmp/b")));

    assert!(update.stats.did_rebuild);
    assert!(!Arc::ptr_eq(&first, &view.roots()[0]));
    assert_eq!(update.stats.rebuilt_roots, 1);
}
