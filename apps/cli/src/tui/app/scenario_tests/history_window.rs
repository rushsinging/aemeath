use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiDisplayHistoryIndex, TuiDisplayHistoryStepReference, TuiResumedSessionStep,
};
use crate::tui::adapter::tui_runtime_event::{TuiRunContext, TuiRuntimeEvent, TuiToolCallStatus};
use crate::tui::effect::effect::Effect;
use crossterm::event::{KeyCode, KeyModifiers};

use super::super::testing::{input, TuiScenarioHarness};

fn context(index: usize) -> TuiRunContext {
    TuiRunContext {
        chat_id: format!("history-chat-{index}"),
        run_id: format!("history-turn-{index}"),
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

fn load_to_sliding_window(harness: &mut TuiScenarioHarness) {
    while harness.app.view_state.output.render_line_limit() < 3_000 {
        harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
        harness.render();
    }
}

fn load_to_oldest_history(harness: &mut TuiScenarioHarness) {
    load_to_sliding_window(harness);
    while harness.app.view_state.output.history_window_tail_offset
        < harness
            .app
            .view_state
            .output
            .source_total_lines
            .saturating_sub(harness.app.view_state.output.render_line_limit())
    {
        harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
        harness.render();
    }
    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();
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
fn resumed_history_initial_window_loads_through_display_query_effect() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: Some(TuiDisplayHistoryIndex {
            session_id: "resume-query".into(),
            generation_revision: 42,
            steps: vec![TuiDisplayHistoryStepReference {
                run_id: "resume-query-run".into(),
                step_id: "resume-query-step".into(),
                member_name: "steps/0001.json".into(),
                estimated_lines: 12,
                user_input_history: Vec::new(),
                finalize_cause: None,
                duration_ms: None,
            }],
        }),
        steps: Vec::new(),
        session_id: "resume-query".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();

    assert!(harness.effects().iter().any(|effect| matches!(
        effect,
        Effect::LoadDisplayHistoryWindow { request }
            if request.session_id == "resume-query"
                && request.generation_revision == 42
                && request.member_names == ["steps/0001.json"]
    )));
    assert!(!harness
        .effects()
        .iter()
        .any(|effect| matches!(effect, Effect::SendChatInputEvent { .. })));
}

#[test]
fn resumed_history_initial_window_keeps_newest_complete_groups() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..1_200)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-tail-run".into(),
            step_id: format!("resume-tail-step-{index}"),
            messages: vec![TuiChatMessage::assistant_text(format!(
                "RESUME-TAIL-ANSWER-{index:04}"
            ))],
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-tail".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();

    let document = harness.app.output_area.document();
    let plain = document
        .iter_lines()
        .map(|line| line.plain.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(plain.contains("RESUME-TAIL-ANSWER-1199"));
    assert!(!plain.contains("RESUME-TAIL-ANSWER-0000"));
    assert!(plain.contains("更早的消息已折叠"));
}

#[test]
fn resumed_history_initial_window_keeps_real_conclusion_tail_shape() {
    let mut harness = TuiScenarioHarness::new(229, 60);
    let old_tool_like_output =
        std::iter::repeat_n(r#"{"type":"tool_result","content":"OLD-TOOL-JSON"}"#, 1_200)
            .collect::<Vec<_>>()
            .join("\n");
    let steps = vec![
        TuiResumedSessionStep {
            run_id: "019f9c58-18a1-7572-bd92-9cd50143736b".into(),
            step_id: "019f9c5e-61fd-7713-8398-3acf508b8d8b".into(),
            messages: vec![TuiChatMessage::assistant_text(old_tool_like_output)],
            finalize_cause: None,
            duration_ms: None,
        },
        TuiResumedSessionStep {
            run_id: "019fa25f-088e-7542-b17e-6c59c659ff22".into(),
            step_id: "019fa25f-088e-7542-b17e-6c6cf52b3781".into(),
            messages: vec![
                TuiChatMessage::user_text("结论呢"),
                TuiChatMessage::assistant_text("#649 还不能关闭，而且剩余工作不少"),
            ],
            finalize_cause: None,
            duration_ms: None,
        },
        TuiResumedSessionStep {
            run_id: "019fa26c-c3a5-7081-8a08-18c2f67f7a3c".into(),
            step_id: "019fa26c-c3a5-7081-8a08-18d590c15043".into(),
            messages: vec![
                TuiChatMessage::user_text("结论呢"),
                TuiChatMessage::assistant_text("#649 现在不能关，下一步只做 #1397"),
            ],
            finalize_cause: None,
            duration_ms: None,
        },
        TuiResumedSessionStep {
            run_id: "019fa29b-ed93-78e2-b933-77a084d4bfa5".into(),
            step_id: "019fa29b-ed94-7ff1-816d-b66ec87c3ed7".into(),
            messages: vec![TuiChatMessage::user_text("继续")],
            finalize_cause: None,
            duration_ms: None,
        },
    ];

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "019f9952-601d-7139-a936-fa5d1f366eb9".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();

    let plain = harness
        .app
        .output_area
        .document()
        .iter_lines()
        .map(|line| line.plain.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let timeline_texts = harness
        .app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::UserMessage {
                text, ..
            }
            | crate::tui::model::output_timeline::OutputTimelineItem::AssistantText {
                text, ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(timeline_texts.ends_with(&[
        "结论呢",
        "#649 还不能关闭，而且剩余工作不少",
        "结论呢",
        "#649 现在不能关，下一步只做 #1397",
        "继续",
    ]));
    assert!(plain.contains("结论呢"));
    assert!(plain.contains("#649 还不能关闭，而且剩余工作不少"));
    assert!(plain.contains("#649 现在不能关，下一步只做 #1397"));
    assert!(plain.contains("继续"));
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
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-oldest".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();
    assert!(harness.app.view_state.output.source_total_lines > 3_000);

    load_to_oldest_history(&mut harness);

    assert_eq!(
        harness.app.view_state.output.history_window_tail_offset,
        harness
            .app
            .view_state
            .output
            .source_total_lines
            .saturating_sub(harness.app.view_state.output.render_line_limit()),
        "到达最早历史后窗口尾部偏移必须达到最早可见位置"
    );
    assert!(
        !harness.screen().contains("更早的消息已折叠"),
        "到达最早历史后不应显示更早消息折叠提示"
    );
    assert!(harness.screen().contains("OLDEST-QUESTION-0000"));
}

#[test]
fn resumed_history_scrolls_from_oldest_window_back_to_latest() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..1_200)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-round-trip-run".into(),
            step_id: format!("resume-round-trip-step-{index}"),
            messages: vec![TuiChatMessage::user_text(format!(
                "ROUND-TRIP-QUESTION-{index:04}"
            ))],
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-round-trip".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();

    load_to_oldest_history(&mut harness);
    assert!(harness.app.view_state.output.history_window_tail_offset > 0);
    assert!(harness.screen().contains("ROUND-TRIP-QUESTION-0000"));

    let mut previous_tail_offset = harness.app.view_state.output.history_window_tail_offset;
    while harness.app.view_state.output.history_window_tail_offset > 0 {
        harness.key(input::press(KeyCode::PageDown, KeyModifiers::NONE));
        harness.render();
        let tail_offset = harness.app.view_state.output.history_window_tail_offset;
        assert!(
            tail_offset <= previous_tail_offset,
            "向下滚动时历史窗口只能向最新方向移动"
        );
        previous_tail_offset = tail_offset;
    }
    while !harness.app.view_state.output.auto_scroll {
        harness.key(input::press(KeyCode::PageDown, KeyModifiers::NONE));
        harness.render();
    }

    assert_eq!(harness.app.view_state.output.history_window_tail_offset, 0);
    assert_eq!(harness.app.view_state.output.scroll_offset, 0);
    assert!(harness.app.view_state.output.auto_scroll);
    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_000);
    assert!(harness.screen().contains("ROUND-TRIP-QUESTION-1199"));
    assert!(!harness.screen().contains("ROUND-TRIP-QUESTION-0000"));
}

#[test]
fn scrolling_to_top_loads_history_by_visible_height() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    seed_history(&mut harness, 1_300);

    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_000);
    assert!(harness
        .app
        .output_area
        .document()
        .blocks
        .iter()
        .any(|block| block.block_id == "_folded_hint"));

    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();

    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_015);
    assert!(
        harness.app.output_area.document().total_lines()
            <= harness
                .app
                .view_state
                .output
                .render_line_limit()
                .saturating_add(3),
        "完整 root group 允许跨过行预算边界，但只能保留一个小的 group 原子性余量"
    );
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
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-large".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();
    assert!(harness.app.view_state.output.source_total_lines > 1_500);
    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_000);

    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();
    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_015);

    harness.key(input::press(KeyCode::Home, KeyModifiers::SHIFT));
    harness.render();
    assert_eq!(harness.app.view_state.output.render_line_limit(), 1_030);
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
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-sliding".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();
    load_to_sliding_window(&mut harness);
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
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-capped".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();
    assert!(harness.app.view_state.output.source_total_lines > 3_000);

    load_to_sliding_window(&mut harness);
    assert_eq!(harness.app.view_state.output.render_line_limit(), 3_000);
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
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();

    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-early".into(),
        created_at: 0,
        compacted: false,
    });
    harness.app.view_state.output.request_load_older_at_top();
    harness.render();

    assert_eq!(
        harness.app.view_state.output.render_line_limit(),
        harness.app.view_state.output.source_total_lines.min(1_015)
    );
}

#[test]
fn adopted_user_message_after_resumed_history_returns_to_latest_window() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..2_000)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-adopted-run".into(),
            step_id: format!("resume-adopted-step-{index}"),
            messages: vec![TuiChatMessage::assistant_text(format!(
                "RESUME-ADOPTED-ANSWER-{index:04}"
            ))],
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-adopted".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();
    load_to_oldest_history(&mut harness);
    assert!(harness.app.view_state.output.history_window_tail_offset > 0);
    assert!(!harness.app.view_state.output.auto_scroll);

    harness.runtime_event(TuiRuntimeEvent::UserMessagesAdopted {
        items: vec![TuiChatMessage {
            role: "user".into(),
            content: vec![crate::tui::adapter::runtime_view::TuiContentBlock::text(
                "RESUME-ADOPTED-NEW-USER",
            )],
            input_id: Some("resume-adopted-input".into()),
            source: crate::tui::adapter::runtime_view::TuiMessageSource::User,
            stop_hook: None,
            skill_request: None,
        }],
        queued: vec![],
    });
    harness.render();

    assert_eq!(harness.app.view_state.output.history_window_tail_offset, 0);
    assert!(harness.app.view_state.output.auto_scroll);
    assert!(harness.screen().contains("RESUME-ADOPTED-NEW-USER"));

    let context = context(9_999);
    harness.runtime_event(TuiRuntimeEvent::Text {
        context: context.clone(),
        text: "RESUME-ADOPTED-NEXT-ASSISTANT".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::BlockComplete {
        context,
        text: "RESUME-ADOPTED-NEXT-ASSISTANT".into(),
    });
    harness.render();
    assert!(harness.screen().contains("RESUME-ADOPTED-NEXT-ASSISTANT"));
}

fn visible_rows_with_prefix(screen: &str, prefix: &str) -> Vec<(usize, String)> {
    screen
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(prefix))
        .map(|(row, line)| (row, line.to_owned()))
        .collect()
}

#[test]
fn sliding_window_rebuild_keeps_visible_history_rows_fixed_in_same_frame() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let steps = (0..2_000)
        .map(|index| TuiResumedSessionStep {
            run_id: "resume-anchor-run".into(),
            step_id: format!("resume-anchor-step-{index}"),
            messages: vec![TuiChatMessage::user_text(format!(
                "ANCHOR-QUESTION-{index:04}"
            ))],
            finalize_cause: None,
            duration_ms: None,
        })
        .collect();
    harness.runtime_event(TuiRuntimeEvent::SessionResumed {
        display_history: None,
        steps,
        session_id: "resume-anchor".into(),
        created_at: 0,
        compacted: false,
    });
    harness.render();
    load_to_sliding_window(&mut harness);

    let before_rows = visible_rows_with_prefix(&harness.screen(), "ANCHOR-QUESTION-");
    assert!(!before_rows.is_empty());
    let before_tail_offset = harness.app.view_state.output.history_window_tail_offset;

    assert!(harness.app.view_state.output.request_load_older_at_top());
    harness.app.mark_output_dirty();
    harness.render();

    assert!(harness.app.view_state.output.history_window_tail_offset > before_tail_offset);
    assert_eq!(
        visible_rows_with_prefix(&harness.screen(), "ANCHOR-QUESTION-"),
        before_rows,
        "历史窗口重建帧必须保持当前可见历史文字的屏幕行不变"
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
