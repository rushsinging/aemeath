use super::RetainedOutputView;
use crate::tui::model::conversation::block::AskUserSlot;
use crate::tui::model::conversation::ids::{ChatId, ChatRunId};
use crate::tui::model::conversation::intent::{
    AppendUserMessage, AssistantText, DismissAskUserBatch, ShowAskUserBatch,
};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::resumed_history::ResumedHistoryBacking;
use crate::tui::view_model::OutputRenderWindow;
use std::sync::Arc;

fn full_window() -> OutputRenderWindow {
    OutputRenderWindow {
        line_limit: usize::MAX,
        tail_offset: 0,
    }
}

fn materialize_all(
    view: &mut RetainedOutputView,
    model: &ConversationModel,
    display_history: &crate::tui::model::display_history::DisplayHistoryModel,
    workspace_root: Option<&std::path::Path>,
) -> super::MaterializedOutputWindow {
    view.materialize_window(model, display_history, workspace_root, full_window())
}

fn root_ids(view: &RetainedOutputView) -> Vec<&str> {
    view.roots()
        .iter()
        .map(|root| root.block_id.as_str())
        .collect()
}

#[test]
fn indexed_resume_requests_only_selected_window_members() {
    let model = ConversationModel::default();
    let mut display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    display_history.replace(ResumedHistoryBacking::from_index(
        sdk::DisplayHistoryIndex {
            session_id: "session-window".to_string(),
            generation_revision: 13,
            steps: (0..100)
                .map(|step_index| sdk::DisplayHistoryStepReference {
                    run_id: format!("run-{step_index}"),
                    step_id: format!("step-{step_index}"),
                    member_name: format!("step-{step_index}.json"),
                    estimated_lines: 10,
                    user_input_history: Vec::new(),
                    finalize_cause: None,
                    duration_ms: None,
                })
                .collect(),
        },
    ));
    let mut view = RetainedOutputView::default();

    let window = view.materialize_window(
        &model,
        &display_history,
        None,
        OutputRenderWindow {
            line_limit: 20,
            tail_offset: 0,
        },
    );

    let request = window
        .missing_history_request
        .expect("selected members must be requested");
    assert_eq!(request.session_id, "session-window");
    assert_eq!(request.generation_revision, 13);
    assert_eq!(request.member_names, ["step-98.json", "step-99.json"]);
    assert!(window.view_model.roots.is_empty());
}

#[test]
fn loaded_display_history_rebuilds_the_same_window_without_conversation_change() {
    let model = ConversationModel::default();
    let mut display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    display_history.replace(ResumedHistoryBacking::from_index(
        sdk::DisplayHistoryIndex {
            session_id: "session-window".to_string(),
            generation_revision: 13,
            steps: vec![sdk::DisplayHistoryStepReference {
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
                member_name: "step-1.json".to_string(),
                estimated_lines: 10,
                user_input_history: Vec::new(),
                finalize_cause: None,
                duration_ms: None,
            }],
        },
    ));
    let mut view = RetainedOutputView::default();
    let request = OutputRenderWindow {
        line_limit: 20,
        tail_offset: 0,
    };
    let initial = view.materialize_window(&model, &display_history, None, request);
    assert!(initial.view_model.roots.is_empty());
    let conversation_revision = model.revision();

    assert!(display_history.apply_window(
        crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow {
            session_id: "session-window".to_string(),
            generation_revision: 13,
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
                messages: vec![
                    crate::tui::adapter::runtime_view::TuiChatMessage::assistant_text(
                        "loaded-tail"
                    ),
                ],
                finalize_cause: None,
                duration_ms: None,
            }],
        },
    ));
    view.invalidate_display_history();
    let loaded = view.materialize_window(&model, &display_history, None, request);

    assert_eq!(model.revision(), conversation_revision);
    assert!(loaded.missing_history_request.is_none());
    assert_eq!(loaded.view_model.roots.len(), 1);
    assert!(loaded.stats.did_rebuild);
}

#[test]
fn cold_window_materializes_only_tail_candidates() {
    let mut model = ConversationModel::default();
    for index in 0..100_000 {
        model.apply(AppendUserMessage {
            text: format!("message-{index}"),
        });
    }
    let display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    let mut view = RetainedOutputView::default();

    let window = view.materialize_window(
        &model,
        &display_history,
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
fn moving_window_retires_roots_outside_current_selection() {
    let mut model = ConversationModel::default();
    for index in 0..128 {
        model.apply(AppendUserMessage {
            text: format!("message-{index}"),
        });
    }
    let display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    let mut view = RetainedOutputView::default();
    let latest = view.materialize_window(
        &model,
        &display_history,
        None,
        OutputRenderWindow {
            line_limit: 10,
            tail_offset: 0,
        },
    );
    assert_eq!(view.cached_root_count(), latest.view_model.roots.len());

    let older = view.materialize_window(
        &model,
        &display_history,
        None,
        OutputRenderWindow {
            line_limit: 10,
            tail_offset: 80,
        },
    );

    assert_eq!(view.cached_root_count(), older.view_model.roots.len());
    assert!(view.cached_root_count() <= 10);
}

#[test]
fn append_reuses_existing_roots_and_creates_only_the_new_root() {
    let mut model = ConversationModel::default();
    for index in 0..128 {
        model.apply(AppendUserMessage {
            text: format!("message-{index}"),
        });
    }
    let display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    let mut view = RetainedOutputView::default();
    let initial = materialize_all(&mut view, &model, &display_history, None);
    let retained = view.roots().to_vec();
    assert_eq!(initial.stats.rebuilt_roots, 128);

    model.apply(AppendUserMessage {
        text: "appended".to_string(),
    });
    let update = materialize_all(&mut view, &model, &display_history, None);

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
        run_id: ChatRunId::new("turn-1"),
        text: "first".to_string(),
    });
    let display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    let mut view = RetainedOutputView::default();
    materialize_all(&mut view, &model, &display_history, None);
    let stable = Arc::clone(&view.roots()[0]);
    let streaming = Arc::clone(&view.roots()[1]);

    model.apply(AssistantText {
        chat_id: ChatId::new("chat-1"),
        run_id: ChatRunId::new("turn-1"),
        text: " second".to_string(),
    });
    let update = materialize_all(&mut view, &model, &display_history, None);

    assert_eq!(update.stats.created_roots, 1);
    assert_eq!(update.stats.touched_roots, 1);
    assert_eq!(update.stats.reused_roots, 1);
    assert!(Arc::ptr_eq(&stable, &view.roots()[0]));
    assert!(!Arc::ptr_eq(&streaming, &view.roots()[1]));
}

#[test]
fn reset_rebuilds_from_the_new_conversation_window() {
    let mut model = ConversationModel::default();
    model.apply(AppendUserMessage {
        text: "before-reset".to_string(),
    });
    let display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    let mut view = RetainedOutputView::default();
    materialize_all(&mut view, &model, &display_history, None);

    model.reset();
    model.apply(AppendUserMessage {
        text: "after-reset".to_string(),
    });
    let update = materialize_all(&mut view, &model, &display_history, None);
    assert!(update.stats.did_rebuild);
    assert_eq!(root_ids(&view), vec!["user-1"]);
    assert_eq!(update.view_model.roots, view.roots());
}

#[test]
fn dismiss_updates_only_the_active_ask_user_root_to_cancel_pending() {
    let mut model = ConversationModel::default();
    model.apply(AppendUserMessage {
        text: "stable".to_string(),
    });
    model.apply(ShowAskUserBatch {
        request_id: crate::tui::model::conversation::interaction::UiInteractionRequestId::from(
            "question-1",
        ),
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
    let display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    let mut view = RetainedOutputView::default();
    materialize_all(&mut view, &model, &display_history, None);
    let stable = Arc::clone(&view.roots()[0]);

    model.apply(DismissAskUserBatch);
    let update = materialize_all(&mut view, &model, &display_history, None);

    assert!(!update.stats.did_rebuild);
    assert_eq!(update.stats.touched_roots, 1);
    assert_eq!(update.stats.reused_roots, 1);
    assert_eq!(view.roots().len(), 2);
    assert!(Arc::ptr_eq(&stable, &view.roots()[0]));
}

#[test]
fn workspace_change_rebuilds_tool_sensitive_roots_once() {
    let mut model = ConversationModel::default();
    model.apply(AppendUserMessage {
        text: "stable".to_string(),
    });
    let display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    let mut view = RetainedOutputView::default();
    materialize_all(
        &mut view,
        &model,
        &display_history,
        Some(std::path::Path::new("/tmp/a")),
    );
    let first = Arc::clone(&view.roots()[0]);

    let update = materialize_all(
        &mut view,
        &model,
        &display_history,
        Some(std::path::Path::new("/tmp/b")),
    );

    assert!(update.stats.did_rebuild);
    assert!(!Arc::ptr_eq(&first, &view.roots()[0]));
    assert_eq!(update.stats.rebuilt_roots, 1);
}
