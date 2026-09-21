//! 输出区渲染产物的值类型：显示 spans 与逻辑 plain 分离。
//!
//! 不变式：每个 `RenderedLine` 的 `plain` 等于其 `spans` 可见文本拼接
//! （见 primitives / blocks 各组件单测断言）。

use std::ops::Range;
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
    pub markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy,
}

impl RenderCtx {
    #[cfg(test)]
    pub const fn for_width(text_width: u16) -> Self {
        Self {
            text_width,
            markdown_spacing: crate::tui::render::output::spacing::MarkdownSpacingPolicy::normal(),
        }
    }
}

/// 行内 link 的位置与 URL，用于 Cmd+Click 打开。
/// `col_start` / `col_end` 是 **plain 文本**中的字符偏移（与 `RenderedLine::plain` 对齐）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkSpan {
    pub col_start: usize,
    pub col_end: usize,
    pub url: String,
}

/// 只在 viewport 绘制阶段解析的行级动画；不进入 `plain`，不改变选择/复制语义。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineAnimation {
    /// 运行中工具首行 gutter 在实心/空心圆之间切换。
    RunningToolMarker,
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
    /// 仅在可视行绘制时按当前 frame 应用的动画元数据。
    pub animation: Option<LineAnimation>,
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
            animation: None,
        }
    }

    /// 构造一条空渲染行。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 从纯文本构造渲染行。
    #[cfg(test)]
    pub fn from_plain(text: impl Into<String>) -> Self {
        let plain = text.into();
        Self {
            spans: vec![Span::raw(plain.clone())],
            plain,
            style: Style::default(),
            gutter_cols: 0,
            fill_style: None,
            links: Vec::new(),
            animation: None,
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
            animation: None,
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
            animation: None,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenderedLineAnchor {
    block_id: String,
    line_offset: usize,
}

/// 整个输出文档的渲染产物（按 block 顺序）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedDocument {
    pub blocks: Vec<RenderedBlock>,
    /// 每个 root group 的 block 数；为空时兼容旧构造方式，每个 block 视为独立 group。
    pub root_group_block_counts: Vec<usize>,
    /// 每个 block 结束后的累计逻辑行数。与 `blocks` 同长度，用于二分定位逻辑行。
    pub(crate) block_line_ends: Vec<usize>,
}

pub struct RenderedLinesInRange<'a> {
    document: &'a RenderedDocument,
    next_index: usize,
    end: usize,
    block_index: usize,
    line_index: usize,
}

impl<'a> Iterator for RenderedLinesInRange<'a> {
    type Item = (usize, &'a RenderedLine);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_index >= self.end {
            return None;
        }
        while self.block_index < self.document.blocks.len() {
            let block = &self.document.blocks[self.block_index];
            if let Some(line) = block.lines.get(self.line_index) {
                let index = self.next_index;
                self.next_index += 1;
                self.line_index += 1;
                return Some((index, line));
            }
            self.block_index += 1;
            self.line_index = 0;
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.end.saturating_sub(self.next_index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for RenderedLinesInRange<'_> {}

impl RenderedDocument {
    #[cfg(test)]
    pub fn new(blocks: Vec<RenderedBlock>) -> Self {
        let block_line_ends = block_line_ends(&blocks);
        Self {
            blocks,
            root_group_block_counts: Vec::new(),
            block_line_ends,
        }
    }

    pub fn with_root_groups(groups: Vec<Vec<RenderedBlock>>) -> Self {
        let root_group_block_counts = groups.iter().map(Vec::len).collect();
        let blocks = groups.into_iter().flatten().collect::<Vec<_>>();
        let block_line_ends = block_line_ends(&blocks);
        Self {
            blocks,
            root_group_block_counts,
            block_line_ends,
        }
    }

    pub fn total_lines(&self) -> usize {
        self.block_line_ends.last().copied().unwrap_or(0)
    }

    #[cfg(test)]
    pub fn iter_lines(&self) -> impl Iterator<Item = &RenderedLine> {
        self.blocks.iter().flat_map(|block| block.lines.iter())
    }

    /// 按文档逻辑行索引查询单行。通过 block 累计行结束索引二分定位，
    /// 不展平或复制整份文档。
    pub fn line_at(&self, index: usize) -> Option<&RenderedLine> {
        let block_index = self.block_line_ends.partition_point(|end| *end <= index);
        let block = self.blocks.get(block_index)?;
        let block_start = block_index
            .checked_sub(1)
            .and_then(|previous| self.block_line_ends.get(previous))
            .copied()
            .unwrap_or(0);
        block.lines.get(index - block_start)
    }

    pub(crate) fn line_anchor_at(&self, index: usize) -> Option<RenderedLineAnchor> {
        let (block_index, line_offset) = self.locate_line(index)?;
        Some(RenderedLineAnchor {
            block_id: self.blocks.get(block_index)?.block_id.clone(),
            line_offset,
        })
    }

    pub(crate) fn stable_line_anchor_in_range(
        &self,
        range: Range<usize>,
    ) -> Option<(RenderedLineAnchor, usize)> {
        let end = range.end.min(self.total_lines());
        for index in range.start.min(end)..end {
            let anchor = self.line_anchor_at(index)?;
            if anchor.block_id != "_folded_hint" {
                return Some((anchor, index.saturating_sub(range.start)));
            }
        }
        None
    }

    pub(crate) fn line_index_for_anchor(&self, anchor: &RenderedLineAnchor) -> Option<usize> {
        let block_index = self
            .blocks
            .iter()
            .position(|block| block.block_id == anchor.block_id)?;
        let block = self.blocks.get(block_index)?;
        if anchor.line_offset >= block.lines.len() {
            return None;
        }
        let block_start = block_index
            .checked_sub(1)
            .and_then(|previous| self.block_line_ends.get(previous))
            .copied()
            .unwrap_or(0);
        Some(block_start.saturating_add(anchor.line_offset))
    }

    /// 迭代逻辑行范围。每次单行查询通过累计行索引二分定位，
    /// 只访问请求区间，不展平完整文档。
    pub fn lines_in_range(&self, range: Range<usize>) -> RenderedLinesInRange<'_> {
        let end = range.end.min(self.total_lines());
        let start = range.start.min(end);
        let (block_index, line_index) = self.locate_line(start).unwrap_or((self.blocks.len(), 0));
        RenderedLinesInRange {
            document: self,
            next_index: start,
            end,
            block_index,
            line_index,
        }
    }

    fn locate_line(&self, index: usize) -> Option<(usize, usize)> {
        let block_index = self.block_line_ends.partition_point(|end| *end <= index);
        let block = self.blocks.get(block_index)?;
        let block_start = block_index
            .checked_sub(1)
            .and_then(|previous| self.block_line_ends.get(previous))
            .copied()
            .unwrap_or(0);
        if index.saturating_sub(block_start) < block.lines.len() {
            Some((block_index, index - block_start))
        } else {
            None
        }
    }

    pub(crate) fn rebuild_line_index(&mut self) {
        self.block_line_ends = block_line_ends(&self.blocks);
    }

    #[cfg(test)]
    pub fn root_group_block_counts(&self) -> Vec<usize> {
        if self.root_group_block_counts.is_empty() {
            vec![1; self.blocks.len()]
        } else {
            self.root_group_block_counts.clone()
        }
    }
}

fn block_line_ends(blocks: &[RenderedBlock]) -> Vec<usize> {
    let mut total = 0usize;
    blocks
        .iter()
        .map(|block| {
            total = total.saturating_add(block.lines.len());
            total
        })
        .collect()
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
        let doc = RenderedDocument::new(vec![
            RenderedBlock {
                block_id: "a".into(),
                lines: Rc::new(vec![RenderedLine::default(), RenderedLine::default()]),
            },
            RenderedBlock {
                block_id: "b".into(),
                lines: Rc::new(vec![RenderedLine::default()]),
            },
        ]);

        assert_eq!(doc.total_lines(), 3);
        assert_eq!(doc.iter_lines().count(), 3);
    }

    #[test]
    fn line_at_crosses_empty_and_non_empty_blocks() {
        let doc = RenderedDocument::new(vec![
            RenderedBlock {
                block_id: "empty".into(),
                lines: Rc::new(Vec::new()),
            },
            RenderedBlock {
                block_id: "first".into(),
                lines: Rc::new(vec![
                    RenderedLine::from_plain("zero"),
                    RenderedLine::from_plain("one"),
                ]),
            },
            RenderedBlock {
                block_id: "second".into(),
                lines: Rc::new(vec![RenderedLine::from_plain("two")]),
            },
        ]);

        assert_eq!(doc.line_at(0).map(|line| line.plain.as_str()), Some("zero"));
        assert_eq!(doc.line_at(1).map(|line| line.plain.as_str()), Some("one"));
        assert_eq!(doc.line_at(2).map(|line| line.plain.as_str()), Some("two"));
        assert_eq!(doc.line_at(3), None);
    }

    #[test]
    fn line_anchor_round_trips_across_prefixed_blocks() {
        let old = RenderedDocument::new(vec![
            RenderedBlock {
                block_id: "a".into(),
                lines: Rc::new(vec![RenderedLine::from_plain("a0")]),
            },
            RenderedBlock {
                block_id: "anchor".into(),
                lines: Rc::new(vec![
                    RenderedLine::from_plain("anchor0"),
                    RenderedLine::from_plain("anchor1"),
                ]),
            },
        ]);
        let expanded = RenderedDocument::new(vec![
            RenderedBlock {
                block_id: "earlier".into(),
                lines: Rc::new(vec![
                    RenderedLine::from_plain("earlier0"),
                    RenderedLine::from_plain("earlier1"),
                ]),
            },
            RenderedBlock {
                block_id: "a".into(),
                lines: Rc::new(vec![RenderedLine::from_plain("a0")]),
            },
            RenderedBlock {
                block_id: "anchor".into(),
                lines: Rc::new(vec![
                    RenderedLine::from_plain("anchor0"),
                    RenderedLine::from_plain("anchor1"),
                ]),
            },
        ]);

        let anchor = old.line_anchor_at(2).expect("old anchor");
        assert_eq!(expanded.line_index_for_anchor(&anchor), Some(4));
    }

    #[test]
    fn lines_in_range_returns_global_indices_across_block_boundaries() {
        let doc = RenderedDocument::new(vec![
            RenderedBlock {
                block_id: "first".into(),
                lines: Rc::new(vec![
                    RenderedLine::from_plain("zero"),
                    RenderedLine::from_plain("one"),
                ]),
            },
            RenderedBlock {
                block_id: "empty".into(),
                lines: Rc::new(Vec::new()),
            },
            RenderedBlock {
                block_id: "second".into(),
                lines: Rc::new(vec![
                    RenderedLine::from_plain("two"),
                    RenderedLine::from_plain("three"),
                ]),
            },
        ]);

        let selected = doc
            .lines_in_range(1..3)
            .map(|(index, line)| (index, line.plain.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(selected, vec![(1, "one"), (2, "two")]);
        assert_eq!(doc.lines_in_range(9..12).count(), 0);
    }
}
