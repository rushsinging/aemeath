//! output_area 常量（#1146 双轨归位）。

use crate::tui::render::theme;
use ratatui::style::Color;

pub(crate) const SPINNER_FRAMES: &[char] = &['·', '✢', '✳', '✶', '✻', '✽', '✻', '✶', '✳', '✢', '·'];

pub(crate) const SPINNER_BASE: Color = theme::SPINNER_BASE;
pub(crate) const SPINNER_HIGHLIGHT: Color = theme::SPINNER_HIGHLIGHT;
pub(crate) const SPINNER_DIM: Color = theme::SPINNER_DIM;

pub(crate) const COMPACT_BAR_MAX_WIDTH: usize = 30;
/// 工具调用详情行的缩进
/// 工具调用详情行的缩进
pub const INDENT: &str = "  ";

/// 默认终端宽度
/// 默认终端宽度
pub const DEFAULT_WIDTH: usize = 120;
