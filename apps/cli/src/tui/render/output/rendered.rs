//! 输出区渲染产物的值类型：显示 spans 与逻辑 plain 分离。
//!
//! 不变式：每个 `RenderedLine` 的 `plain` 等于其 `spans` 可见文本拼接
//! （见 primitives / blocks 各组件单测断言）。

use std::rc::Rc;

use ratatui::style::Style;
use ratatui::text::Span;

/// 渲染管线的渲染上下文。
///
/// 当前主题是编译期 `render::theme` 常量，无运行时 Theme，故只持宽度。
/// TODO(theme): 引入运行时主题后加 `theme` 字段并把 theme_version 纳入 CacheKey。
///
/// 渲染上下文（按 block 传递）。
///
/// **#329 语义约定**：`text_width` 是 **block 文本可用宽度**（已扣除组合期注入的 gutter），
/// 不是输出文档外层宽度。`document_renderer::render_node` 必须用
/// `gutter::effective_block_width(outer_width, depth)` 转换后再塞进 ctx。
/// block 内部用 `ctx.text_width` 做 wrap，wrap 后 line 加回 gutter 总可见宽 ≤ outer。
///
/// `gutter_cols` 仅做尾部空白填充，已在 `apply_gutter_with_frame` 阶段处理，
/// 不影响 wrap 宽度。
#[derive(Clone, Copy, Debug)]
pub struct RenderCtx {
    pub text_width: u16,
}

/// 行内 link 的位置与 URL，用于 Cmd+Click 打开。
/// `col_start` / `col_end` 是 **plain 文本**中的字符偏移（与 `RenderedLine::plain` 对齐）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkSpan {
    pub col_start: usize,
    pub col_end: usize,
    pub url: String,
}

/// 一行渲染产物。`spans` 用于显示（含 markdown/语法/theme 色），
/// `plain` 是逻辑纯文本（选中/复制用）。
///
/// `gutter_cols` 记录前导 gutter 占用的显示列 / span 字符数（gutter 不进 plain）。
/// 选区高亮与点击列→plain 字符映射据此补偿（gutter 是 chrome，不参与选中/复制）。
///
/// `links` 记录行内 link 的位置与 URL，用于 Cmd+Click 打开。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedLine {
    pub spans: Vec<Span<'static>>,
    pub plain: String,
    /// 行级 base style（对应 ratatui `Line::style`）。span 未显式设置的属性会继承此值。
    pub style: Style,
    /// 前导 gutter 的显示列数（亦即 spans 首部 gutter 字符数）。无 gutter 时为 0。
    pub gutter_cols: usize,
    /// 整条可见行的填充样式。由最终 buffer render 负责填满行宽，不进入 plain。
    pub fill_style: Option<Style>,
    /// 行内 link 的 (col_start, col_end, url) 列表（偏移与 plain 对齐）。
    pub links: Vec<LinkSpan>,
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
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links: Vec::new(),
        }
    }

    /// 构造一条空渲染行。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 从纯文本构造渲染行。
    pub fn from_plain(text: impl Into<String>) -> Self {
        let plain = text.into();
        Self {
            spans: vec![Span::raw(plain.clone())],
            plain,
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links: Vec::new(),
        }
    }

    /// 显式提供 plain（用于 markdown 等显示文本 ≠ 逻辑文本的场景）。
    pub fn with_plain(spans: Vec<Span<'static>>, plain: String) -> Self {
        Self {
            spans,
            plain,
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links: Vec::new(),
        }
    }

    /// 显式提供 plain 和 links（用于 markdown link Cmd+Click）。
    pub fn with_plain_and_links(
        spans: Vec<Span<'static>>,
        plain: String,
        links: Vec<LinkSpan>,
    ) -> Self {
        Self {
            spans,
            plain,
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links,
        }
    }

    /// 设置行级 base style（span 未显式设置的属性会继承此值）。
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// 设置整行填充样式。
    pub fn with_fill_style(mut self, style: Style) -> Self {
        self.fill_style = Some(style);
        self
    }

    /// 原地设置整行填充样式。
    pub fn set_fill_style(&mut self, style: Style) {
        self.fill_style = Some(style);
    }
}

/// 一个 block 的渲染产物（多行）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedBlock {
    pub block_id: String,
    pub lines: Rc<Vec<RenderedLine>>,
}

impl RenderedBlock {
    /// 为 block 内所有行设置统一填充样式。
    pub fn with_line_fill_style(mut self, style: Style) -> Self {
        for line in Rc::make_mut(&mut self.lines) {
            line.set_fill_style(style);
        }
        self
    }
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
    fn test_rendered_line_with_fill_style_preserves_plain_text() {
        let fill = Style::default().bg(Color::Blue);
        let line = RenderedLine::from_plain("hello").with_fill_style(fill);

        assert_eq!(line.plain, "hello");
        assert_eq!(line.fill_style, Some(fill));
    }

    #[test]
    fn test_rendered_line_empty_with_fill_style_has_no_filler_text() {
        let fill = Style::default().bg(Color::Blue);
        let line = RenderedLine::empty().with_fill_style(fill);

        assert_eq!(line.plain, "");
        assert!(line.spans.is_empty());
        assert_eq!(line.fill_style, Some(fill));
    }

    #[test]
    fn test_rendered_document_total_lines_sums_blocks() {
        let doc = RenderedDocument {
            blocks: vec![
                RenderedBlock {
                    block_id: "a".into(),
                    lines: Rc::new(vec![RenderedLine::default(), RenderedLine::default()]),
                },
                RenderedBlock {
                    block_id: "b".into(),
                    lines: Rc::new(vec![RenderedLine::default()]),
                },
            ],
        };

        assert_eq!(doc.total_lines(), 3);
        assert_eq!(doc.iter_lines().count(), 3);
    }
}
