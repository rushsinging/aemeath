//! tool coordination 层行为常量（#1146 组3a 常量归位）。
//!
//! `loop_guard` 的循环防护阈值统一归位于此；值与语义不变。

pub(crate) const RECENT_TOOL_CALL_LIMIT: usize = 64;
pub(crate) const CONSECUTIVE_TOOL_CALL_SOFT_LIMIT: usize = 3;
pub(crate) const CONSECUTIVE_TOOL_CALL_HARD_LIMIT: usize = 5;
pub(crate) const PERIOD_MIN_LEN: usize = 2;
pub(crate) const PERIOD_MAX_LEN: usize = 5;
pub(crate) const PERIOD_REPEAT_LIMIT: usize = 3;
pub(crate) const TOOL_FUSE_HARD_PAUSE_LIMIT: usize = 3;
pub(crate) const MAX_INPUT_SUMMARY_CHARS: usize = 160;
