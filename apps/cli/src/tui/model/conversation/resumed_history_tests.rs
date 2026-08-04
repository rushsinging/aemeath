use super::resumed_history::{ResumedHistoryBacking, ResumedHistoryItemKind};
use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock, TuiMessageSource};
use crate::tui::view_model::OutputBlockKind;

fn index(session_id: &str, revision: u64, count: usize) -> sdk::DisplayHistoryIndex {
    sdk::DisplayHistoryIndex {
        session_id: session_id.to_string(),
        generation_revision: revision,
        steps: (0..count)
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
    }
}

fn window(
    session_id: &str,
    revision: u64,
    step_index: usize,
) -> crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow {
    crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow {
        session_id: session_id.to_string(),
        generation_revision: revision,
        steps: vec![crate::tui::adapter::runtime_view::TuiResumedSessionStep {
            run_id: format!("run-{step_index}"),
            step_id: format!("step-{step_index}"),
            messages: vec![
                crate::tui::adapter::runtime_view::TuiChatMessage::user_text(format!(
                    "body-{step_index}"
                )),
            ],
            finalize_cause: None,
            duration_ms: None,
        }],
    }
}

#[test]
fn index_requests_only_missing_selected_members() {
    let mut backing = ResumedHistoryBacking::from_index(index("session", 7, 3));
    let ids = vec!["history-step-1".to_string(), "history-step-2".to_string()];

    let first = backing
        .history_window_request(&ids)
        .expect("window request");
    assert_eq!(first.session_id, "session");
    assert_eq!(first.generation_revision, 7);
    assert_eq!(first.member_names, ["step-1.json", "step-2.json"]);

    assert!(backing.apply_window(window("session", 7, 1)));
    let second = backing
        .history_window_request(&ids)
        .expect("remaining request");
    assert_eq!(second.member_names, ["step-2.json"]);
}

#[test]
fn loaded_window_replaces_step_placeholder_with_renderable_items() {
    let mut backing = ResumedHistoryBacking::from_index(index("session", 7, 2));
    assert_eq!(backing.items().len(), 2);

    assert!(backing.apply_window(window("session", 7, 1)));

    assert_eq!(backing.items().len(), 2);
    assert_eq!(backing.items()[0].id, "history-step-0");
    assert_eq!(backing.items()[1].id, "history-1-message-0");
}

#[test]
fn loaded_window_excludes_llm_only_user_role_messages() {
    let mut backing = ResumedHistoryBacking::from_index(index("session", 7, 1));
    let mut stop_hook = TuiChatMessage::system_generated_user_text(
        "<system-reminder>blocked by hook</system-reminder>",
    );
    stop_hook.source = TuiMessageSource::StopHook;
    stop_hook.stop_hook = None;
    let mut loaded = window("session", 7, 0);
    loaded.steps[0].messages = vec![
        TuiChatMessage::user_text("visible user input"),
        stop_hook,
        TuiChatMessage::system_generated_user_text(
            "<system-reminder>Skill loaded</system-reminder>",
        ),
        TuiChatMessage {
            role: "assistant".to_string(),
            content: vec![TuiContentBlock::text("assistant reply")],
            source: TuiMessageSource::User,
            stop_hook: None,
            skill_request: None,
            input_id: None,
        },
    ];

    assert!(backing.apply_window(loaded));

    assert_eq!(
        backing
            .items()
            .iter()
            .filter(|item| matches!(item.kind, ResumedHistoryItemKind::UserMessage { .. }))
            .count(),
        1
    );
    assert!(backing.user_input_history().is_empty());
}

#[test]
fn loaded_stop_hook_window_assembles_dedicated_feedback_block() {
    let mut display_history = crate::tui::model::display_history::DisplayHistoryModel::default();
    display_history.replace(ResumedHistoryBacking::from_index(index("session", 7, 1)));
    let feedback = crate::tui::adapter::runtime_view::TuiStopHookFeedback {
        summary: "Stop hook prevented stopping.".to_string(),
        command: "check-agent-stop.sh".to_string(),
        exit_code: Some(1),
        reason: "exit code 1".to_string(),
        stdout_preview: "stdout".to_string(),
        stderr_preview: "stderr".to_string(),
        stdout_truncated: false,
        stderr_truncated: false,
        output_file: None,
    };
    let mut loaded = window("session", 7, 0);
    loaded.steps[0].messages = vec![TuiChatMessage::stop_hook_feedback(
        "model feedback",
        feedback,
    )];
    assert!(display_history.apply_window(loaded));

    let item = display_history
        .items()
        .iter()
        .find(|item| {
            matches!(
                item.kind,
                ResumedHistoryItemKind::TypedJson {
                    source: super::resumed_history::TypedJsonHistorySource::StopHook,
                    ..
                }
            )
        })
        .expect("Stop Hook history item");
    let block = crate::tui::view_assembler::resumed_history::assemble_resumed_history_item(
        &display_history,
        item,
    )
    .expect("Stop Hook block");

    assert!(matches!(block.kind, OutputBlockKind::StopHookFeedback(_)));
}

#[test]
fn loaded_window_projects_skill_and_stop_hook_as_typed_json_not_user_messages() {
    let mut backing = ResumedHistoryBacking::from_index(index("session", 7, 1));
    let mut loaded = window("session", 7, 0);
    loaded.steps[0].messages = vec![
        TuiChatMessage::user_text("visible user input"),
        TuiChatMessage::skill_request(
            "LLM skill prompt",
            crate::tui::adapter::runtime_view::TuiSkillRequestMetadata {
                skill: "superpowers:brainstorming".to_string(),
                arguments: "feature scope".to_string(),
                raw_input: "/superpowers:brainstorming feature scope".to_string(),
            },
        ),
        TuiChatMessage::stop_hook_feedback(
            "LLM hook prompt",
            crate::tui::adapter::runtime_view::TuiStopHookFeedback {
                summary: "Stop hook prevented stopping.".to_string(),
                command: ".agents/hooks/check-agent-stop.sh".to_string(),
                exit_code: Some(2),
                reason: "guard failed".to_string(),
                stdout_preview: "details".to_string(),
                stderr_preview: "blocked".to_string(),
                stdout_truncated: false,
                stderr_truncated: false,
                output_file: None,
            },
        ),
    ];

    assert!(backing.apply_window(loaded));

    assert_eq!(
        backing
            .items()
            .iter()
            .filter(|item| matches!(item.kind, ResumedHistoryItemKind::UserMessage { .. }))
            .count(),
        1
    );
    assert!(backing.items().iter().any(|item| {
        matches!(
            item.kind,
            ResumedHistoryItemKind::TypedJson {
                source: super::resumed_history::TypedJsonHistorySource::SkillRequest,
                ..
            }
        )
    }));
    assert!(backing.items().iter().any(|item| {
        matches!(
            item.kind,
            ResumedHistoryItemKind::TypedJson {
                source: super::resumed_history::TypedJsonHistorySource::StopHook,
                ..
            }
        )
    }));
}

#[test]
fn stale_window_cannot_pollute_replaced_session() {
    let mut backing = ResumedHistoryBacking::from_index(index("new-session", 11, 2));

    assert!(!backing.apply_window(window("old-session", 10, 0)));
    assert!(!backing.apply_window(window("new-session", 10, 0)));
    assert_eq!(backing.loaded_step_count(), 0);
}

#[test]
fn loaded_step_cache_is_bounded() {
    let mut backing = ResumedHistoryBacking::from_index(index("session", 9, 160));
    for step_index in 0..160 {
        assert!(backing.apply_window(window("session", 9, step_index)));
    }

    assert!(backing.loaded_step_count() <= 128);
    assert!(backing.step(0).is_none());
    assert!(backing.step(159).is_some());
    assert_eq!(backing.items()[0].id, "history-step-0");
}
