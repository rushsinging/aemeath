use super::performance::*;
use std::time::Duration;

#[test]
fn capture_when_scope_active_returns_accumulated_snapshot() {
    let (_value, snapshot) = capture(|| {
        record_assemble(9, 7, Duration::from_micros(13));
        record_viewport_render(100, 20, Duration::from_micros(17));
        record_terminal_draw(Duration::from_micros(19));
        record_terminal_diff(23, Duration::from_micros(11));
        record_backend_flush(Duration::from_micros(5));
        record_document_render(Duration::from_micros(7));
        record_document_render(Duration::from_micros(5));
        record_edit_diff(Duration::from_micros(3));
        record_diff_build(4, Duration::from_micros(2));
        record_syntax_highlighter_creation();
        record_syntax_highlight(11, Duration::from_micros(1));
        record_block_cache_hit();
        record_block_cache_miss();
        record_block_cache_absent_miss();
        record_block_cache_version_miss();
        record_block_cache_width_miss();
        record_block_cache_spacing_miss();
        record_block_cache_retain_evictions(2);
        record_gutted_cache_hit();
        record_gutted_cache_miss();
        record_gutted_cache_absent_miss();
        record_gutted_cache_version_miss();
        record_gutted_cache_width_miss();
        record_gutted_cache_depth_miss();
        record_gutted_cache_spacing_miss();
        record_gutted_cache_retain_evictions(3);
        42
    });

    assert_eq!(snapshot.assemble_calls, 1);
    assert_eq!(snapshot.assemble_ns, 13_000);
    assert_eq!(snapshot.assemble_source_items, 9);
    assert_eq!(snapshot.assemble_output_roots, 7);
    assert_eq!(snapshot.viewport_render_calls, 1);
    assert_eq!(snapshot.viewport_render_ns, 17_000);
    assert_eq!(snapshot.viewport_source_lines, 100);
    assert_eq!(snapshot.viewport_visible_lines, 20);
    assert_eq!(snapshot.terminal_draw_calls, 1);
    assert_eq!(snapshot.terminal_draw_ns, 19_000);
    assert_eq!(snapshot.terminal_diff_calls, 1);
    assert_eq!(snapshot.terminal_diff_ns, 11_000);
    assert_eq!(snapshot.terminal_diff_cells, 23);
    assert_eq!(snapshot.backend_flush_calls, 1);
    assert_eq!(snapshot.backend_flush_ns, 5_000);
    assert_eq!(snapshot.document_render_calls, 2);
    assert_eq!(snapshot.document_render_ns, 12_000);
    assert_eq!(snapshot.edit_diff_calls, 1);
    assert_eq!(snapshot.edit_diff_ns, 3_000);
    assert_eq!(snapshot.diff_build_calls, 1);
    assert_eq!(snapshot.diff_build_output_lines, 4);
    assert_eq!(snapshot.diff_build_ns, 2_000);
    assert_eq!(snapshot.syntax_highlighter_creations, 1);
    assert_eq!(snapshot.syntax_highlight_calls, 1);
    assert_eq!(snapshot.syntax_highlight_input_bytes, 11);
    assert_eq!(snapshot.syntax_highlight_ns, 1_000);
    assert_eq!(snapshot.block_cache_hits, 1);
    assert_eq!(snapshot.block_cache_misses, 1);
    assert_eq!(snapshot.block_cache_absent_misses, 1);
    assert_eq!(snapshot.block_cache_version_misses, 1);
    assert_eq!(snapshot.block_cache_width_misses, 1);
    assert_eq!(snapshot.block_cache_spacing_misses, 1);
    assert_eq!(snapshot.block_cache_retain_evictions, 2);
    assert_eq!(snapshot.gutted_cache_hits, 1);
    assert_eq!(snapshot.gutted_cache_misses, 1);
    assert_eq!(snapshot.gutted_cache_absent_misses, 1);
    assert_eq!(snapshot.gutted_cache_version_misses, 1);
    assert_eq!(snapshot.gutted_cache_width_misses, 1);
    assert_eq!(snapshot.gutted_cache_depth_misses, 1);
    assert_eq!(snapshot.gutted_cache_spacing_misses, 1);
    assert_eq!(snapshot.gutted_cache_retain_evictions, 3);
}

#[test]
fn record_when_scope_inactive_is_noop() {
    record_document_render(Duration::from_micros(7));
    record_syntax_highlight(11, Duration::from_micros(1));

    let (_, snapshot) = capture(|| ());

    assert_eq!(snapshot, RenderPerformanceSnapshot::default());
}

#[test]
fn panic_inside_capture_clears_thread_local_scope() {
    let panic_result = std::panic::catch_unwind(|| {
        capture(|| panic!("fixture panic"));
    });
    assert!(panic_result.is_err());
    assert!(!is_active(), "panic 后 capture scope 必须清理");

    let (_, snapshot) = capture(record_block_cache_hit);
    assert_eq!(snapshot.block_cache_hits, 1);
}

#[test]
fn percentiles_sort_samples_and_use_nearest_rank() {
    assert_eq!(percentiles_ns(&[]), None);
    assert_eq!(percentiles_ns(&[7]), Some((7, 7)));
    assert_eq!(percentiles_ns(&[50, 10, 40, 20, 30]), Some((30, 50)));
    assert_eq!(
        percentiles_ns(&(1..=20).rev().collect::<Vec<_>>()),
        Some((10, 19))
    );
}
