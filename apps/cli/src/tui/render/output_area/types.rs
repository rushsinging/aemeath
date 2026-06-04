/// 最大保留行数
pub const MAX_LINES: usize = 10000;

/// 默认终端宽度
pub const DEFAULT_WIDTH: usize = 120;

/// 工具调用详情行的缩进
pub const INDENT: &str = "  ";

use ratatui::style::Color;

/// 带颜色的一段文本，用于行内分段着色（如 diff 语法高亮）。
///
/// 这是 `render::syntax` 与 `render::output::diff` 着色原语的中间单元，
/// 经 `primitives::convert::spanparts_to_spans` 转为 `RenderedLine`。
#[derive(Clone, Debug)]
pub struct SpanPart {
    pub text: String,
    pub color: Color,
}

impl SpanPart {
    pub fn plain(text: impl Into<String>, color: Color) -> Self {
        Self {
            text: text.into(),
            color,
        }
    }
}
