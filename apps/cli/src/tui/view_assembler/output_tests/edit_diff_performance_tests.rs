use super::super::{assemble_output_view, assemble_output_window};
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::intent::{
    AppendSystemMessage, StartChat, ToolCallUpdate, ToolResult,
};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::render::output::document_renderer::OutputDocumentRenderer;
use crate::tui::render::output::spacing::MarkdownSpacingPolicy;
use crate::tui::render::performance::{capture, percentiles_ns, RenderPerformanceSnapshot};
use crate::tui::view_model::OutputBlockKind;
use crate::tui::view_model::OutputRenderWindow;
use std::time::Instant;

fn source_lines(count: usize, changed_index: Option<usize>) -> String {
    (0..count)
        .map(|index| {
            if changed_index == Some(index) {
                format!("fn item_{index}() {{ println!(\"新值 {index} ✓\"); }}")
            } else if index + 1 == count {
                format!(
                    "fn item_{index}() {{ println!(\"{}\"); }}",
                    "x".repeat(4096)
                )
            } else {
                format!("fn item_{index}() {{ println!(\"旧值 {index} — Unicode\"); }}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn edit_conversation(edit_count: usize, lines_per_diff: usize) -> ConversationModel {
    let mut conversation = ConversationModel::default();
    conversation.apply(StartChat {
        submission: "恢复长会话并检查 Edit 历史".to_string(),
    });
    let chat_id = ChatId::new("chat-main");
    let turn_id = ChatTurnId::new("turn-main");

    for index in 0..edit_count {
        let id = ToolCallId::new(format!("edit-{index}"));
        let provider_id = format!("provider-edit-{index}");
        let old = source_lines(lines_per_diff, None);
        let new = source_lines(lines_per_diff, Some(lines_per_diff / 2));
        let arguments = serde_json::json!({
            "file_path": format!("src/generated_{index}.rs"),
            "old_string": "old",
            "new_string": "new"
        })
        .to_string();
        conversation.apply(ToolCallUpdate {
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            id: id.clone(),
            provider_id: Some(provider_id.clone()),
            name: "Edit".to_string(),
            index,
            arguments: Some(arguments),
            status: ToolCallStatus::Ready,
        });
        conversation.apply(ToolResult {
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            id,
            provider_id,
            tool_name: "Edit".to_string(),
            output: format!("replaced 1 occurrence(s) in src/generated_{index}.rs"),
            content: serde_json::json!({
                "file_path": format!("src/generated_{index}.rs"),
                "replacements_made": 1,
                "dry_run": false,
                "old": old,
                "new": new,
                "start_line": 1
            }),
            is_error: false,
            image_count: 0,
        });
    }
    conversation
}

fn tool_result_count(vm: &crate::tui::view_model::output::OutputViewModel) -> usize {
    vm.roots
        .iter()
        .flat_map(|root| root.children.iter())
        .filter(|child| matches!(child.kind, OutputBlockKind::ToolResult(_)))
        .count()
}

#[test]
fn edit_fixture_assembles_expected_tool_result_children() {
    let conversation = edit_conversation(4, 20);
    let vm = assemble_output_view(&conversation, None);
    assert_eq!(tool_result_count(&vm), 4);
}

#[test]
fn edit_cold_work_scales_and_spinner_warm_render_reuses_static_diff() {
    fn render(edit_count: usize) -> (RenderPerformanceSnapshot, RenderPerformanceSnapshot) {
        let conversation = edit_conversation(edit_count, 40);
        let vm = assemble_output_view(&conversation, None);
        let mut renderer = OutputDocumentRenderer::default();
        let (_, cold) = capture(|| {
            renderer.render_model_document(&vm, 100, 100, 0, MarkdownSpacingPolicy::normal())
        });
        let (_, warm) = capture(|| {
            renderer.render_model_document(&vm, 100, 100, 1, MarkdownSpacingPolicy::normal())
        });
        (cold, warm)
    }

    let (small, _) = render(2);
    let (large, warm) = render(6);

    assert_eq!(small.edit_diff_calls, 2);
    assert_eq!(large.edit_diff_calls, 6);
    assert!(large.diff_build_output_lines > small.diff_build_output_lines);
    assert!(large.syntax_highlight_calls > small.syntax_highlight_calls);
    assert_eq!(warm.edit_diff_calls, 0);
    assert_eq!(warm.diff_build_calls, 0);
    assert_eq!(warm.syntax_highlight_calls, 0);
    assert_eq!(
        warm.gutted_cache_hits, 13,
        "用户 root + 每个 Edit 的 ToolCall/ToolResult"
    );
}

#[test]
fn edit_cold_window_limits_diff_work_to_selected_history() {
    fn render(edit_count: usize) -> RenderPerformanceSnapshot {
        let conversation = edit_conversation(edit_count, 100);
        let vm = assemble_output_window(
            &conversation,
            None,
            OutputRenderWindow {
                line_limit: 100,
                tail_offset: 0,
            },
        );
        let mut renderer = OutputDocumentRenderer::default();
        let (_, metrics) = capture(|| {
            renderer.render_tree_with_window(
                &vm,
                100,
                0,
                MarkdownSpacingPolicy::normal(),
                OutputRenderWindow {
                    line_limit: 100,
                    tail_offset: 0,
                },
            )
        });
        metrics
    }

    let small = render(10);
    let large = render(100);

    assert!(small.edit_diff_calls <= 10);
    assert!(large.edit_diff_calls <= 25);
    assert!(large.edit_diff_calls < 100);
    assert!(large.syntax_highlighter_creations <= large.edit_diff_calls);
    assert!(large.syntax_highlight_calls < 20_000);
}

#[test]
fn revision_update_after_history_trim_reuses_windowed_static_edit_layout() {
    let mut conversation = edit_conversation(6, 2_000);
    let mut renderer = OutputDocumentRenderer::default();
    let first_vm = assemble_output_view(&conversation, None);
    let (_, cold) = capture(|| {
        renderer.render_tree_with_window(
            &first_vm,
            100,
            0,
            MarkdownSpacingPolicy::normal(),
            OutputRenderWindow {
                line_limit: 10_000,
                tail_offset: 0,
            },
        )
    });
    conversation.apply(AppendSystemMessage {
        text: "与既有 Edit 内容无关的新消息".to_string(),
    });
    let next_vm = assemble_output_view(&conversation, None);
    let (_, revised) = capture(|| {
        renderer.render_tree_with_window(
            &next_vm,
            100,
            1,
            MarkdownSpacingPolicy::normal(),
            OutputRenderWindow {
                line_limit: 10_000,
                tail_offset: 0,
            },
        )
    });
    assert_eq!(
        cold.block_cache_retain_evictions, 0,
        "窗口裁剪不再逐出语义上仍存活的 block cache"
    );
    assert_eq!(
        cold.gutted_cache_retain_evictions, 0,
        "窗口裁剪不再逐出语义上仍存活的 gutted cache"
    );
    assert_eq!(revised.edit_diff_calls, 0);
    assert_eq!(revised.diff_build_calls, 0);
    assert_eq!(revised.syntax_highlight_calls, 0);
}

#[test]
#[ignore = "性能验收；手动运行：cargo test -p cli --release edit_diff_window_release_workload -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
fn edit_diff_window_release_workload() {
    const SAMPLES: usize = 20;
    const WINDOW_LINES: usize = 1_000;
    println!(
        "\n=== #1420 Edit diff 窗口化验收（width=100, window_lines={WINDOW_LINES}, samples={SAMPLES}）==="
    );

    for edit_count in [10usize, 50, 100] {
        let conversation = edit_conversation(edit_count, 2_000);
        let vm = assemble_output_view(&conversation, None);
        let mut cold_ns = Vec::with_capacity(SAMPLES);
        let mut representative = RenderPerformanceSnapshot::default();

        for sample in 0..SAMPLES {
            let mut renderer = OutputDocumentRenderer::default();
            let ((), cold) = capture(|| {
                let started = Instant::now();
                let _ = renderer.render_tree_with_window(
                    &vm,
                    100,
                    0,
                    MarkdownSpacingPolicy::normal(),
                    OutputRenderWindow {
                        line_limit: WINDOW_LINES,
                        tail_offset: 0,
                    },
                );
                cold_ns.push(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
            });
            assert_eq!(
                cold.edit_diff_calls, 1,
                "冷启动 diff 工作量必须受历史窗口约束"
            );
            if sample == 0 {
                representative = cold;
            }
        }

        let (cold_p50, cold_p95) = percentiles_ns(&cold_ns).unwrap();
        println!(
            "edits={edit_count:>3} total_source_lines={:>6} | cold_p50/p95={:.2}/{:.2}ms diff_calls={} diff_output_lines={} highlighter_creations={} highlight_calls={} highlight_bytes={}",
            edit_count * 2_000,
            cold_p50 as f64 / 1_000_000.0,
            cold_p95 as f64 / 1_000_000.0,
            representative.edit_diff_calls,
            representative.diff_build_output_lines,
            representative.syntax_highlighter_creations,
            representative.syntax_highlight_calls,
            representative.syntax_highlight_input_bytes,
        );
    }
}

#[test]
#[ignore = "性能基线；手动运行：cargo test -p cli --release edit_diff_release_workload -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
fn edit_diff_release_workload() {
    const SAMPLES: usize = 20;
    println!("\n=== #1418 Edit diff 性能基线（width=100, samples={SAMPLES}）===");

    for (edit_count, lines_per_diff) in [(5, 20), (10, 50), (10, 100)] {
        let conversation = edit_conversation(edit_count, lines_per_diff);
        let mut assemble_ns = Vec::with_capacity(SAMPLES);
        let mut cold_ns = Vec::with_capacity(SAMPLES);
        let mut warm_ns = Vec::with_capacity(SAMPLES);
        let mut representative = RenderPerformanceSnapshot::default();

        for sample in 0..SAMPLES {
            let started = Instant::now();
            let vm = assemble_output_view(&conversation, None);
            assemble_ns.push(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));

            let mut renderer = OutputDocumentRenderer::default();
            let ((), cold) = capture(|| {
                let started = Instant::now();
                let _ = renderer.render_model_document(
                    &vm,
                    100,
                    100,
                    0,
                    MarkdownSpacingPolicy::normal(),
                );
                cold_ns.push(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
            });
            let ((), warm) = capture(|| {
                let started = Instant::now();
                let _ = renderer.render_model_document(
                    &vm,
                    100,
                    100,
                    1,
                    MarkdownSpacingPolicy::normal(),
                );
                warm_ns.push(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
            });
            if sample == 0 {
                representative = cold;
                assert_eq!(warm.syntax_highlight_calls, 0);
            }
        }

        let (assemble_p50, assemble_p95) = percentiles_ns(&assemble_ns).unwrap();
        let (cold_p50, cold_p95) = percentiles_ns(&cold_ns).unwrap();
        let (warm_p50, warm_p95) = percentiles_ns(&warm_ns).unwrap();
        println!(
            "edits={edit_count:>2} lines_per_diff={lines_per_diff:>4} total_source_lines={:>5} | assemble_p50/p95={:.2}/{:.2}ms cold_p50/p95={:.2}/{:.2}ms warm_p50/p95={:.3}/{:.3}ms | diff_calls={} diff_output_lines={} highlighter_creations={} highlight_calls={} highlight_bytes={} block_miss={} block_absent={} block_version={} block_width={} block_spacing={} block_evicted={} gutted_miss={} gutted_absent={} gutted_version={} gutted_width={} gutted_depth={} gutted_spacing={} gutted_evicted={}",
            edit_count * lines_per_diff,
            assemble_p50 as f64 / 1_000_000.0,
            assemble_p95 as f64 / 1_000_000.0,
            cold_p50 as f64 / 1_000_000.0,
            cold_p95 as f64 / 1_000_000.0,
            warm_p50 as f64 / 1_000_000.0,
            warm_p95 as f64 / 1_000_000.0,
            representative.edit_diff_calls,
            representative.diff_build_output_lines,
            representative.syntax_highlighter_creations,
            representative.syntax_highlight_calls,
            representative.syntax_highlight_input_bytes,
            representative.block_cache_misses,
            representative.block_cache_absent_misses,
            representative.block_cache_version_misses,
            representative.block_cache_width_misses,
            representative.block_cache_spacing_misses,
            representative.block_cache_retain_evictions,
            representative.gutted_cache_misses,
            representative.gutted_cache_absent_misses,
            representative.gutted_cache_version_misses,
            representative.gutted_cache_width_misses,
            representative.gutted_cache_depth_misses,
            representative.gutted_cache_spacing_misses,
            representative.gutted_cache_retain_evictions,        );
    }
}
