use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiResumedSessionStep};
use crate::tui::adapter::tui_runtime_event::{TuiRuntimeEvent, TuiToolCallStatus, TuiTurnContext};
use crossterm::event::{KeyCode, KeyModifiers};

use super::super::testing::{input, TuiScenarioHarness};

fn context(index: usize) -> TuiTurnContext {
    TuiTurnContext {
        chat_id: format!("history-chat-{index}"),
        turn_id: format!("history-turn-{index}"),
    }
}

fn seed_history(harness: &mut TuiScenarioHarness, count: usize) {
    for index in 0..count {
        let context = context(index);
        let text = format!("HISTORY-{index:04}");
        harness.runtime_event(TuiRuntimeEvent::Text {
            context: context.clone(),
            text: text.clone(),
        });
        harness.runtime_event(TuiRuntimeEvent::BlockComplete { context, text });
    }
    harness.render();
}

fn seed_edit(harness: &mut TuiScenarioHarness, index: usize, diff_lines: usize) {
    let context = context(index);
    let id = format!("edit-{index}");
    let old = (0..diff_lines)
        .map(|line| format!("old-{index}-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let new = (0..diff_lines)
        .map(|line| format!("new-{index}-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    harness.runtime_event(TuiRuntimeEvent::ToolCallStart {
        context: context.clone(),
        id: id.clone(),
        provider_id: Some(id.clone()),
        name: "Edit".into(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallUpdate {
        context: context.clone(),
        id: id.clone(),
        provider_id: Some(id.clone()),
        name: "Edit".into(),
        index: 0,
        arguments_delta: None,
        arguments: Some(serde_json::json!({
            "file_path": format!("src/edit-{index}.rs"),
            "old_string": old,
            "new_string": new,
        })),
        status: TuiToolCallStatus::Ready,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolResult {
        context,
        id: id.clone(),
        provider_id: id,
        tool_name: "Edit".into(),
        output: format!("EDIT-RESULT-{index}"),
        content: serde_json::json!({
            "old": old,
            "new": new,
            "start_line": 1,
            "path": format!("src/edit-{index}.rs"),
        }),
        is_error: false,
        images: vec![],
    });
}

fn assert_tool_groups_are_complete(harness: &TuiScenarioHarness) {
    let document = harness.app.output_area.document();
    let counts = document.root_group_block_counts();
    let mut start = 0;
    for count in counts {
        let group = &document.blocks[start..start + count];
        let has_edit_call = group
            .iter()
            .any(|block| block.block_id.starts_with("edit-"));
        let has_edit_result = group.iter().any(|block| {
            block.block_id.starts_with("edit-") && block.block_id.ends_with("-result")
        });
        assert_eq!(
            has_edit_call,
            has_edit_result,
            "Edit ToolCall 与 ToolResult 必须同时保留，group={:?}",
            group
                .iter()
                .map(|block| &block.block_id)
                .collect::<Vec<_>>()
        );
        if has_edit_call {
            assert_eq!(
                count, 2,
                "Edit root group 必须包含 call 与 result 两个 block"
            );
        }
        start += count;
    }
}

#[test]
fn resumed_history_window_never_splits_edit_call_and_diff_result() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    for index in 0..180 {
        seed_edit(&mut harness, index, 16);
    }
    harness.render();
    assert!(harness.app.view_state.output.source_total_lines > 3_000);
    assert_tool_groups_are_complete(&harness);

    for _ in 0..8 {
        harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
        harness.render();
        assert_tool_groups_are_complete(&harness);
    }
}

#[test]
fn resumed_history_window_reaches_oldest_history_without_folded_hint() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..1_200)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-oldest-run".into(),
            step_id: format!("resume-oldest-step-{index}"),
            messages: vec![TuiChatMessage::user_text(format!(
                "OLDEST-QUESTION-{index:04}"
            ))],
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps,
        session_id: "resume-oldest".into(),
        created_at: 0,
    });
    harness.render();
    assert!(harness.app.view_state.output.source_total_lines > 3_000);

    for _ in 0..20 {
        harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
        harness.render();
    }

    assert_eq!(
        harness.app.view_state.output.history_window_tail_offset, 0,
        "到达最早历史后窗口起点必须为 0"
    );
    assert!(
        !harness.screen().contains("更早的消息已折叠"),
        "到达最早历史后不应显示更早消息折叠提示"
    );
    assert!(harness.screen().contains("OLDEST-QUESTION-0000"));
}

#[test]
fn scrolling_to_top_loads_history_in_five_hundred_line_batches() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    seed_history(&mut harness, 1_300);

    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_000);
    assert!(!harness
        .app
        .output_area
        .document()
        .blocks
        .iter()
        .any(|block| block.block_id == "_folded_hint"));

    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();

    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_500);
    assert!(harness.app.output_area.document().total_lines() <= 1_501);
}

#[test]
fn resumed_large_history_loads_after_first_render_and_continues_in_batches() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..600)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-large-run".into(),
            step_id: format!("resume-step-{index}"),
            messages: vec![
                TuiChatMessage::user_text(format!("RESUME-QUESTION-{index:04}")),
                TuiChatMessage::assistant_text(format!("RESUME-ANSWER-{index:04}")),
            ],
        })
        .collect();

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps,
        session_id: "resume-large".into(),
        created_at: 0,
    });
    harness.render();
    assert!(harness.app.view_state.output.source_total_lines > 1_500);
    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_000);

    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();
    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_500);

    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();
    assert_eq!(harness.app.view_state.output.render_line_limit(), 2_000);
}

#[test]
fn resumed_history_window_can_continue_loading_older_blocks_after_cap() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..2_000)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-sliding-run".into(),
            step_id: format!("resume-sliding-step-{index}"),
            messages: vec![
                TuiChatMessage::user_text(format!("SLIDING-QUESTION-{index:04}")),
                TuiChatMessage::assistant_text(format!("SLIDING-ANSWER-{index:04}")),
            ],
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps,
        session_id: "resume-sliding".into(),
        created_at: 0,
    });
    harness.render();
    for _ in 0..4 {
        harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
        harness.render();
    }
    assert_eq!(harness.app.view_state.output.render_line_limit(), 3_000);

    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();
    assert_eq!(harness.app.view_state.output.render_line_limit(), 3_000);
    assert!(harness.screen().contains("SLIDING-QUESTION-"));
}

#[test]
fn resumed_history_window_stops_at_three_thousand_lines() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..1_200)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-capped-run".into(),
            step_id: format!("resume-capped-step-{index}"),
            messages: vec![
                TuiChatMessage::user_text(format!("CAPPED-QUESTION-{index:04}")),
                TuiChatMessage::assistant_text(format!("CAPPED-ANSWER-{index:04}")),
            ],
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps,
        session_id: "resume-capped".into(),
        created_at: 0,
    });
    harness.render();
    assert!(harness.app.view_state.output.source_total_lines > 3_000);

    for expected in [1_500, 2_000, 2_500, 3_000] {
        harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
        harness.render();
        assert_eq!(harness.app.view_state.output.render_line_limit(), expected);
    }
    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();
    assert_eq!(harness.app.view_state.output.render_line_limit(), 3_000);
}

#[test]
fn top_request_before_first_resume_render_loads_after_source_is_observed() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..600)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-early-run".into(),
            step_id: format!("resume-early-step-{index}"),
            messages: vec![TuiChatMessage::assistant_text(format!(
                "RESUME-EARLY-{index:04}"
            ))],
        })
        .collect();

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        steps,
        session_id: "resume-early".into(),
        created_at: 0,
    });
    harness.app.view_state.output.request_load_older_at_top();
    harness.render();

    assert_eq!(
        harness.app.view_state.output.render_line_limit(),
        harness.app.view_state.output.source_total_lines.min(1_500)
    );
}

#[test]
fn streaming_output_does_not_move_scrolled_viewport() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    seed_history(&mut harness, 150);
    harness.key(input::press(KeyCode::PageUp, KeyModifiers::NONE));
    harness.render();
    let before = harness.screen();
    let before_offset = harness.app.view_state.output.scroll_offset;

    let context = context(9999);
    harness.runtime_event(TuiRuntimeEvent::Text {
        context: context.clone(),
        text: "STREAMING-NEW-CONTENT".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::BlockComplete {
        context,
        text: "STREAMING-NEW-CONTENT".into(),
    });
    harness.render();

    assert_eq!(harness.screen(), before);
    assert!(harness.app.view_state.output.scroll_offset > before_offset);
    assert!(!harness.app.view_state.output.auto_scroll);

    harness.key(input::press(KeyCode::End, KeyModifiers::SHIFT));
    harness.render();
    assert!(harness.screen().contains("STREAMING-NEW-CONTENT"));
    assert!(harness.app.view_state.output.auto_scroll);
}
