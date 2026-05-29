//! 输出区渲染产物的值类型：显示 spans 与逻辑 plain 分离。
//!
//! 不变式：每个 `RenderedLine` 的 `plain` 等于其 `spans` 可见文本拼接
//! （见 primitives / blocks 各组件单测断言）。

use ratatui::text::Span;

/// 渲染管线的渲染上下文。
///
/// 当前主题是编译期 `render::theme` 常量，无运行时 Theme，故只持宽度。
/// TODO(theme): 引入运行时主题后加 `theme` 字段并把 theme_version 纳入 CacheKey。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderCtx {
    pub width: u16,
}

/// 一行渲染产物。`spans` 用于显示（含 markdown/语法/theme 色），
/// `plain` 是逻辑纯文本（选中/复制用）。
///
/// `gutter_cols` 记录前导 gutter 占用的显示列 / span 字符数（gutter 不进 plain）。
/// 选区高亮与点击列→plain 字符映射据此补偿（gutter 是 chrome，不参与选中/复制）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedLine {
    pub spans: Vec<Span<'static>>,
    pub plain: String,
    /// 前导 gutter 的显示列数（亦即 spans 首部 gutter 字符数）。无 gutter 时为 0。
    pub gutter_cols: usize,
}

impl RenderedLine {
    /// 从 spans 构造，`plain` 由 spans 可见文本拼接得到。
    pub fn new(spans: Vec<Span<'static>>) -> Self {
        let plain = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        Self {
            spans,
            plain,
            gutter_cols: 0,
        }
    }

    /// 显式提供 plain（用于 markdown 等显示文本 ≠ 逻辑文本的场景）。
    pub fn with_plain(spans: Vec<Span<'static>>, plain: String) -> Self {
        Self {
            spans,
            plain,
            gutter_cols: 0,
        }
    }
}

/// 一个 block 的渲染产物（多行）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedBlock {
    pub block_id: String,
    pub lines: Vec<RenderedLine>,
}

/// 整个输出文档的渲染产物（按 block 顺序）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedDocument {
    pub blocks: Vec<RenderedBlock>,
}

impl RenderedDocument {
    pub fn total_lines(&self) -> usize {
        self.blocks.iter().map(|block| block.lines.len()).sum()
    }

    pub fn iter_lines(&self) -> impl Iterator<Item = &RenderedLine> {
        self.blocks.iter().flat_map(|block| block.lines.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};
    use ratatui::text::Span;

    #[test]
    fn test_rendered_line_new_derives_plain_from_spans() {
        let line = RenderedLine::new(vec![
            Span::styled("Hello ", Style::default().fg(Color::Red)),
            Span::styled("世界", Style::default().fg(Color::Blue)),
        ]);

        assert_eq!(line.plain, "Hello 世界");
        assert_eq!(line.spans.len(), 2);
    }

    #[test]
    fn test_rendered_line_with_plain_keeps_explicit_plain() {
        let line = RenderedLine::with_plain(vec![Span::raw("**x**")], "x".to_string());

        assert_eq!(line.plain, "x");
    }

    #[test]
    fn test_rendered_document_total_lines_sums_blocks() {
        let doc = RenderedDocument {
            blocks: vec![
                RenderedBlock {
                    block_id: "a".into(),
                    lines: vec![RenderedLine::default(), RenderedLine::default()],
                },
                RenderedBlock {
                    block_id: "b".into(),
                    lines: vec![RenderedLine::default()],
                },
            ],
        };

        assert_eq!(doc.total_lines(), 3);
        assert_eq!(doc.iter_lines().count(), 3);
    }
}
