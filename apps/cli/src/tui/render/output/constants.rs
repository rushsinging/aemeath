//! output 子域常量（#1146 双轨归位）。

use crate::tui::render::theme;
use ratatui::style::Color;

pub const GUTTER_WIDTH: usize = 2;
pub(crate) const PER_DEPTH_INDENT: usize = 2;
pub const TOOL_MARKER_BLINK_DIVISOR: u64 = 4;
pub(crate) const NARROW_NO_INDENT_THRESHOLD: u16 = 50;
pub(crate) const NARROW_NO_GUTTER_THRESHOLD: u16 = 30;
pub const NARROW_STATUS_HINT_THRESHOLD: u16 = 40;
pub const NARROW_DISABLE_TABLE_THRESHOLD: u16 = 60;
pub(crate) const MAX_GUTTER_DEPTH: usize = 256;
pub(crate) const DEFAULT_RENDER_CACHE_CAPACITY: usize = 4_096;

pub(crate) const DIFF_REMOVE_FG: Color = theme::DIFF_REMOVE_FG;

pub(crate) const DIFF_ADD_FG: Color = theme::DIFF_ADD_FG;

/// Diff 行号 / 高亮颜色常量。
/// Diff 行号 / 高亮颜色常量。
pub(crate) const LINE_NUM_COLOR: Color = theme::TEXT_DIM;

// ─── markdown.rs ───
// 或以常见代码文件扩展名结尾。
pub(crate) const EXTENSIONS: &[&str] = &[
    ".rs", ".toml", ".md", ".json", ".yaml", ".yml", ".txt", ".sh", ".py", ".ts", ".js", ".tsx",
    ".jsx", ".go", ".c", ".h", ".cpp", ".hpp",
];
