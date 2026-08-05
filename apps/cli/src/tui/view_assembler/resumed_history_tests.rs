use super::{
    assemble_resumed_history_display_units, assemble_resumed_history_item,
    resumed_history_candidate, ResumedHistoryItemKind,
};
use crate::tui::model::conversation::resumed_history::ResumedHistoryBacking;
use crate::tui::model::display_history::DisplayHistoryModel;
use crate::tui::view_assembler::tool_group::{plan_display_units, DisplayUnitPlan};
use crate::tui::view_model::output::ToolGroupKind;

fn history_index() -> sdk::DisplayHistoryIndex {
    sdk::DisplayHistoryIndex {
        session_id: "resume-session".into(),
        generation_revision: 3,
        steps: vec![sdk::DisplayHistoryStepReference {
            run_id: "run-1".into(),
            step_id: "step-1".into(),
            member_name: "step-1.json".into(),
            estimated_lines: 20,
            user_input_history: Vec::new(),
            finalize_cause: None,
            duration_ms: None,
        }],
    }
}

fn loaded_tool_window() -> crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow {
    crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow {
        session_id: "resume-session".into(),
        generation_revision: 3,
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: "run-1".into(),
            step_id: "step-1".into(),
            messages: vec![crate::tui::adapter::runtime_view::TuiChatMessage {
                role: "assistant".into(),
                content: vec![
                    crate::tui::adapter::runtime_view::TuiContentBlock::ToolUse {
                        id: "provider-tool-1".into(),
                        name: "Read".into(),
                        input: serde_json::json!({}),
                    },
                    crate::tui::adapter::runtime_view::TuiContentBlock::ToolUse {
                        id: "provider-tool-2".into(),
                        name: "Glob".into(),
                        input: serde_json::json!({}),
                    },
                ],
                source: crate::tui::adapter::runtime_view::TuiMessageSource::User,
                hook_notice: None,
                skill_request: None,
                input_id: None,
            }],
            finalize_cause: None,
            duration_ms: None,
        }],
    }
}

fn loaded_tool_result_window() -> crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow {
    let mut window = loaded_tool_window();
    window.steps[0]
        .messages
        .push(crate::tui::adapter::runtime_view::TuiChatMessage {
            role: "user".into(),
            content: vec![
                crate::tui::adapter::runtime_view::TuiContentBlock::ToolResult {
                    tool_use_id: "provider-tool-1".into(),
                    content: serde_json::json!("first result"),
                    is_error: false,
                    text: Some("first result".into()),
                },
                crate::tui::adapter::runtime_view::TuiContentBlock::ToolResult {
                    tool_use_id: "provider-tool-2".into(),
                    content: serde_json::json!("second result"),
                    is_error: false,
                    text: Some("second result".into()),
                },
            ],
            source: crate::tui::adapter::runtime_view::TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
            input_id: None,
        });
    window
}

#[test]
fn loaded_resume_display_units_attach_each_result_to_its_matching_tool_call() {
    let mut display_history = DisplayHistoryModel::default();
    display_history.replace(ResumedHistoryBacking::from_index(history_index()));
    assert!(display_history.apply_window(loaded_tool_result_window()));

    let roots = assemble_resumed_history_display_units(&display_history);

    assert_eq!(roots.len(), 1);
    let group = &roots[0];
    assert!(matches!(
        group.kind,
        crate::tui::view_model::OutputBlockKind::ToolGroup(_)
    ));
    assert_eq!(group.children.len(), 2);
    assert!(group.children[0].block_id.contains("provider-tool-1"));
    assert!(group.children[1].block_id.contains("provider-tool-2"));
    assert_eq!(group.children[0].children.len(), 1);
    assert_eq!(group.children[1].children.len(), 1);
}

#[test]
fn placeholder_remains_non_materialized_until_resume_step_is_loaded() {
    let mut display_history = DisplayHistoryModel::default();
    display_history.replace(ResumedHistoryBacking::from_index(history_index()));
    let placeholder = display_history.items().first().expect("placeholder");

    assert!(matches!(
        placeholder.kind,
        ResumedHistoryItemKind::StepPlaceholder
    ));
    assert!(assemble_resumed_history_item(&display_history, placeholder).is_none());
}

#[test]
fn loaded_resume_step_groups_consecutive_tool_calls_by_provider_identity() {
    let mut display_history = DisplayHistoryModel::default();
    display_history.replace(ResumedHistoryBacking::from_index(history_index()));
    assert!(display_history.apply_window(loaded_tool_window()));

    let candidates = display_history
        .items()
        .iter()
        .filter(|item| matches!(item.kind, ResumedHistoryItemKind::ToolCall { .. }))
        .map(|item| resumed_history_candidate(&display_history, item))
        .collect::<Vec<_>>();

    assert_eq!(
        plan_display_units(&candidates),
        vec![DisplayUnitPlan::ToolGroup {
            group_id: "tool-group:step-1:provider-tool-1".into(),
            kind: ToolGroupKind::Explore,
            member_ids: vec!["provider-tool-1".into(), "provider-tool-2".into()],
            attached_results: Vec::new(),
        }]
    );
}
