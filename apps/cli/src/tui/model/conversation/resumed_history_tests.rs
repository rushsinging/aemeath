use super::resumed_history::ResumedHistoryBacking;

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
