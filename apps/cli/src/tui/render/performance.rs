use std::cell::RefCell;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RenderPerformanceSnapshot {
    pub assemble_calls: u64,
    pub assemble_ns: u64,
    pub assemble_source_items: u64,
    pub assemble_output_roots: u64,
    pub retained_view_sync_calls: u64,
    pub retained_view_touched_roots: u64,
    pub retained_view_created_roots: u64,
    pub retained_view_reused_roots: u64,
    pub retained_view_rebuilt_roots: u64,
    pub viewport_render_calls: u64,
    pub viewport_render_ns: u64,
    pub viewport_source_lines: u64,
    pub viewport_visible_lines: u64,
    pub terminal_draw_calls: u64,
    pub terminal_draw_ns: u64,
    pub terminal_diff_calls: u64,
    pub terminal_diff_ns: u64,
    pub terminal_diff_cells: u64,
    pub backend_flush_calls: u64,
    pub backend_flush_ns: u64,
    pub document_render_calls: u64,
    pub document_render_ns: u64,
    pub edit_diff_calls: u64,
    pub edit_diff_ns: u64,
    pub diff_build_calls: u64,
    pub diff_build_ns: u64,
    pub diff_build_output_lines: u64,
    pub syntax_highlighter_creations: u64,
    pub syntax_highlight_calls: u64,
    pub syntax_highlight_ns: u64,
    pub syntax_highlight_input_bytes: u64,
    pub block_cache_hits: u64,
    pub block_cache_misses: u64,
    pub block_cache_absent_misses: u64,
    pub block_cache_version_misses: u64,
    pub block_cache_width_misses: u64,
    pub block_cache_spacing_misses: u64,
    pub block_cache_retain_evictions: u64,
    pub gutted_cache_hits: u64,
    pub gutted_cache_misses: u64,
    pub gutted_cache_absent_misses: u64,
    pub gutted_cache_version_misses: u64,
    pub gutted_cache_width_misses: u64,
    pub gutted_cache_depth_misses: u64,
    pub gutted_cache_spacing_misses: u64,
    pub gutted_cache_retain_evictions: u64,
}

thread_local! {
    static ACTIVE_CAPTURE: RefCell<Option<RenderPerformanceSnapshot>> = const { RefCell::new(None) };
}

struct CaptureGuard {
    active: bool,
}

impl CaptureGuard {
    fn start() -> Self {
        ACTIVE_CAPTURE.with(|capture| {
            assert!(
                capture.borrow().is_none(),
                "render performance capture 不支持嵌套"
            );
            *capture.borrow_mut() = Some(RenderPerformanceSnapshot::default());
        });
        Self { active: true }
    }

    fn finish(mut self) -> RenderPerformanceSnapshot {
        self.active = false;
        ACTIVE_CAPTURE.with(|capture| {
            capture
                .borrow_mut()
                .take()
                .expect("render performance capture scope 应保持有效")
        })
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if self.active {
            ACTIVE_CAPTURE.with(|capture| {
                capture.borrow_mut().take();
            });
        }
    }
}

pub(crate) fn capture<T>(run: impl FnOnce() -> T) -> (T, RenderPerformanceSnapshot) {
    let guard = CaptureGuard::start();
    let value = run();
    (value, guard.finish())
}

pub(crate) fn is_active() -> bool {
    ACTIVE_CAPTURE.with(|capture| capture.borrow().is_some())
}

fn update(update: impl FnOnce(&mut RenderPerformanceSnapshot)) {
    ACTIVE_CAPTURE.with(|capture| {
        if let Some(snapshot) = capture.borrow_mut().as_mut() {
            update(snapshot);
        }
    });
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

pub(crate) fn record_assemble(source_items: usize, output_roots: usize, duration: Duration) {
    update(|snapshot| {
        snapshot.assemble_calls += 1;
        snapshot.assemble_ns = snapshot.assemble_ns.saturating_add(duration_ns(duration));
        snapshot.assemble_source_items = snapshot
            .assemble_source_items
            .saturating_add(u64::try_from(source_items).unwrap_or(u64::MAX));
        snapshot.assemble_output_roots = snapshot
            .assemble_output_roots
            .saturating_add(u64::try_from(output_roots).unwrap_or(u64::MAX));
    });
}

pub(crate) fn record_retained_view_sync(
    touched_roots: usize,
    created_roots: usize,
    reused_roots: usize,
    rebuilt_roots: usize,
) {
    update(|snapshot| {
        snapshot.retained_view_sync_calls += 1;
        snapshot.retained_view_touched_roots = snapshot
            .retained_view_touched_roots
            .saturating_add(u64::try_from(touched_roots).unwrap_or(u64::MAX));
        snapshot.retained_view_created_roots = snapshot
            .retained_view_created_roots
            .saturating_add(u64::try_from(created_roots).unwrap_or(u64::MAX));
        snapshot.retained_view_reused_roots = snapshot
            .retained_view_reused_roots
            .saturating_add(u64::try_from(reused_roots).unwrap_or(u64::MAX));
        snapshot.retained_view_rebuilt_roots = snapshot
            .retained_view_rebuilt_roots
            .saturating_add(u64::try_from(rebuilt_roots).unwrap_or(u64::MAX));
    });
}

pub(crate) fn record_viewport_render(
    source_lines: usize,
    visible_lines: usize,
    duration: Duration,
) {
    update(|snapshot| {
        snapshot.viewport_render_calls += 1;
        snapshot.viewport_render_ns = snapshot
            .viewport_render_ns
            .saturating_add(duration_ns(duration));
        snapshot.viewport_source_lines = snapshot
            .viewport_source_lines
            .saturating_add(u64::try_from(source_lines).unwrap_or(u64::MAX));
        snapshot.viewport_visible_lines = snapshot
            .viewport_visible_lines
            .saturating_add(u64::try_from(visible_lines).unwrap_or(u64::MAX));
    });
}

pub(crate) fn record_terminal_draw(duration: Duration) {
    update(|snapshot| {
        snapshot.terminal_draw_calls += 1;
        snapshot.terminal_draw_ns = snapshot
            .terminal_draw_ns
            .saturating_add(duration_ns(duration));
    });
}

pub(crate) fn record_terminal_diff(cells: usize, duration: Duration) {
    update(|snapshot| {
        snapshot.terminal_diff_calls += 1;
        snapshot.terminal_diff_ns = snapshot
            .terminal_diff_ns
            .saturating_add(duration_ns(duration));
        snapshot.terminal_diff_cells = snapshot
            .terminal_diff_cells
            .saturating_add(u64::try_from(cells).unwrap_or(u64::MAX));
    });
}

pub(crate) fn record_backend_flush(duration: Duration) {
    update(|snapshot| {
        snapshot.backend_flush_calls += 1;
        snapshot.backend_flush_ns = snapshot
            .backend_flush_ns
            .saturating_add(duration_ns(duration));
    });
}

pub(crate) fn record_document_render(duration: Duration) {
    update(|snapshot| {
        snapshot.document_render_calls += 1;
        snapshot.document_render_ns = snapshot
            .document_render_ns
            .saturating_add(duration_ns(duration));
    });
}

pub(crate) fn record_edit_diff(duration: Duration) {
    update(|snapshot| {
        snapshot.edit_diff_calls += 1;
        snapshot.edit_diff_ns = snapshot.edit_diff_ns.saturating_add(duration_ns(duration));
    });
}

pub(crate) fn record_diff_build(output_lines: usize, duration: Duration) {
    update(|snapshot| {
        snapshot.diff_build_calls += 1;
        snapshot.diff_build_ns = snapshot.diff_build_ns.saturating_add(duration_ns(duration));
        snapshot.diff_build_output_lines = snapshot
            .diff_build_output_lines
            .saturating_add(u64::try_from(output_lines).unwrap_or(u64::MAX));
    });
}

pub(crate) fn record_syntax_highlighter_creation() {
    update(|snapshot| snapshot.syntax_highlighter_creations += 1);
}

pub(crate) fn record_syntax_highlight(input_bytes: usize, duration: Duration) {
    update(|snapshot| {
        snapshot.syntax_highlight_calls += 1;
        snapshot.syntax_highlight_ns = snapshot
            .syntax_highlight_ns
            .saturating_add(duration_ns(duration));
        snapshot.syntax_highlight_input_bytes = snapshot
            .syntax_highlight_input_bytes
            .saturating_add(u64::try_from(input_bytes).unwrap_or(u64::MAX));
    });
}

pub(crate) fn record_block_cache_hit() {
    update(|snapshot| snapshot.block_cache_hits += 1);
}

pub(crate) fn record_block_cache_miss() {
    update(|snapshot| snapshot.block_cache_misses += 1);
}

pub(crate) fn record_block_cache_absent_miss() {
    update(|snapshot| snapshot.block_cache_absent_misses += 1);
}

pub(crate) fn record_block_cache_version_miss() {
    update(|snapshot| snapshot.block_cache_version_misses += 1);
}

pub(crate) fn record_block_cache_width_miss() {
    update(|snapshot| snapshot.block_cache_width_misses += 1);
}

pub(crate) fn record_block_cache_spacing_miss() {
    update(|snapshot| snapshot.block_cache_spacing_misses += 1);
}

pub(crate) fn record_block_cache_retain_evictions(count: usize) {
    update(|snapshot| {
        snapshot.block_cache_retain_evictions = snapshot
            .block_cache_retain_evictions
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
    });
}

pub(crate) fn record_gutted_cache_hit() {
    update(|snapshot| snapshot.gutted_cache_hits += 1);
}

pub(crate) fn record_gutted_cache_miss() {
    update(|snapshot| snapshot.gutted_cache_misses += 1);
}

pub(crate) fn record_gutted_cache_absent_miss() {
    update(|snapshot| snapshot.gutted_cache_absent_misses += 1);
}

pub(crate) fn record_gutted_cache_version_miss() {
    update(|snapshot| snapshot.gutted_cache_version_misses += 1);
}

pub(crate) fn record_gutted_cache_width_miss() {
    update(|snapshot| snapshot.gutted_cache_width_misses += 1);
}

pub(crate) fn record_gutted_cache_depth_miss() {
    update(|snapshot| snapshot.gutted_cache_depth_misses += 1);
}

pub(crate) fn record_gutted_cache_spacing_miss() {
    update(|snapshot| snapshot.gutted_cache_spacing_misses += 1);
}

pub(crate) fn record_gutted_cache_retain_evictions(count: usize) {
    update(|snapshot| {
        snapshot.gutted_cache_retain_evictions = snapshot
            .gutted_cache_retain_evictions
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
    });
}

pub(crate) fn percentiles_ns(samples: &[u64]) -> Option<(u64, u64)> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    Some((nearest_rank(&sorted, 50), nearest_rank(&sorted, 95)))
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100).max(1);
    sorted[rank - 1]
}
