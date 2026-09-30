//! output 子域常量（#1146 双轨归位）。

pub const GUTTER_WIDTH: usize = 2;
pub(crate) const PER_DEPTH_INDENT: usize = 2;
pub const TOOL_MARKER_BLINK_DIVISOR: u64 = 4;
pub(crate) const NARROW_NO_INDENT_THRESHOLD: u16 = 50;
pub(crate) const NARROW_NO_GUTTER_THRESHOLD: u16 = 30;
pub const NARROW_STATUS_HINT_THRESHOLD: u16 = 40;
pub const NARROW_DISABLE_TABLE_THRESHOLD: u16 = 60;
pub(crate) const MAX_GUTTER_DEPTH: usize = 256;
