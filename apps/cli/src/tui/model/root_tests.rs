use super::*;

#[test]
fn display_history_window_loading_does_not_change_conversation_revision() {
    let mut model = TuiModel::default();
    model.display_history.replace(
        crate::tui::model::conversation::resumed_history::ResumedHistoryBacking::from_index(
            sdk::DisplayHistoryIndex {
                session_id: "display-session".to_string(),
                generation_revision: 7,
                steps: vec![sdk::DisplayHistoryStepReference {
                    run_id: "run-1".to_string(),
                    step_id: "step-1".to_string(),
                    member_name: "step-1.json".to_string(),
                    estimated_lines: 1,
                    user_input_history: Vec::new(),
                    finalize_cause: None,
                    duration_ms: None,
                }],
            },
        ),
    );
    let conversation_revision = model.conversation.revision();

    assert!(model.display_history.apply_window(
        crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow {
            session_id: "display-session".to_string(),
            generation_revision: 7,
            steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
                messages: vec![
                    crate::tui::adapter::runtime_view::TuiChatMessage::assistant_text("loaded"),
                ],
                finalize_cause: None,
                duration_ms: None,
            }],
        },
    ));
    assert_eq!(model.conversation.revision(), conversation_revision);
}

#[test]
fn test_tui_model_default_has_no_active_chat() {
    let model = TuiModel::default();
    assert!(model.conversation.active_chat_id.is_none());
}

#[test]
fn test_tui_model_default_has_no_prompt() {
    let model = TuiModel::default();
    assert!(model.diagnostic.active_prompt.is_none());
}

#[test]
fn test_tui_model_default_has_empty_input() {
    let model = TuiModel::default();
    assert!(model.input.document.buffer.is_empty());
}
