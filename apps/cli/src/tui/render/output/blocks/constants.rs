//! blocks 子域常量（#1146 双轨归位）。

pub(crate) const LEGACY_DIFF_MARKER: &str = "---DIFF---";
pub(crate) const DIFF_MARKER_PREFIX: &str = "---DIFF";
pub(crate) const DIFF_MARKER_SUFFIX: &str = "---";
pub(crate) const DIFF_LINE_PREFIX: &str = ":LINE:";

pub(crate) const HIGHLIGHT_MAX_SIDE_LINES: usize = 20_000;
pub(crate) const HIGHLIGHT_MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const HIGHLIGHT_MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const RENDER_MAX_SIDE_LINES: usize = 100_000;
pub(crate) const RENDER_MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const RENDER_MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const RETAINED_LINES_PER_END: usize = 250;
pub(crate) const OMITTED_LINE_COUNT_LIMIT: usize = 10_000;

// ─── edit_diff.rs ───
pub(crate) const LABEL_RESERVE_BYTES: usize = 128;
