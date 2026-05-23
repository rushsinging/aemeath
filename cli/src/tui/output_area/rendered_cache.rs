//! 渲染缓存层：滑动窗口 + 增量渲染。

use ratatui::text::Line;

use aemeath_core::string_idx::CharIdx;

use super::rendered_lines;
use super::OutputLine;

/// 单行的渲染结果
#[derive(Clone, Debug)]
pub struct RenderedLine {
    /// 渲染后的 ratatui Line
    pub line: Line<'static>,
    /// 用于 screen_map 的信息：(逻辑行索引, char_start, char_end)
    pub screen_entries: Vec<(usize, CharIdx, CharIdx)>,
    /// 渲染后的文本（与原始 content 不同时，selection 使用）
    pub rendered_text: Option<String>,
}

/// 渲染缓存
pub struct RenderedCache {
    cache: Vec<Option<RenderedLine>>,
    render_start: usize,
    render_end: usize,
    cached_width: usize,
    dirty: bool,
}

impl RenderedCache {
    pub fn new() -> Self {
        Self {
            cache: Vec::new(),
            render_start: 0,
            render_end: 0,
            cached_width: 0,
            dirty: true,
        }
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub fn content_changed(&mut self, total_lines: usize) {
        self.dirty = true;
        if self.cache.len() > total_lines {
            self.cache.truncate(total_lines);
        }
    }

    /// 确保渲染窗口覆盖 [display_start, display_end)，前后各扩展 50%，
    /// 并扩展到 block 边界。
    pub fn ensure_rendered(
        &mut self,
        lines: &[OutputLine],
        display_start: usize,
        display_end: usize,
        term_width: usize,
    ) {
        let total = lines.len();
        if total == 0 || display_start >= display_end {
            return;
        }

        let width_changed = term_width != self.cached_width;
        if width_changed {
            self.dirty = true;
            self.cached_width = term_width;
        }

        let display_size = display_end - display_start;
        let expand = (display_size / 2).max(5);
        let raw_start = display_start.saturating_sub(expand);
        let raw_end = (display_end + expand).min(total);

        let (rs, re) = if self.dirty {
            (raw_start, raw_end)
        } else if raw_start < self.render_start || raw_end > self.render_end {
            (raw_start, raw_end)
        } else {
            return;
        };

        let block_start = expand_to_block_start(lines, rs);
        let block_end = expand_to_block_end(lines, re);

        if self.dirty {
            self.cache.clear();
            self.cache.resize_with(total, || None);
            rendered_lines::render_range(
                lines,
                block_start,
                block_end,
                term_width,
                &mut self.cache,
            );
            self.render_start = block_start;
            self.render_end = block_end;
            self.dirty = false;
        } else {
            self.cache.resize_with(total, || None);
            if block_start < self.render_start {
                rendered_lines::render_range(
                    lines,
                    block_start,
                    self.render_start,
                    term_width,
                    &mut self.cache,
                );
                self.render_start = block_start;
            }
            if block_end > self.render_end {
                rendered_lines::render_range(
                    lines,
                    self.render_end,
                    block_end,
                    term_width,
                    &mut self.cache,
                );
                self.render_end = block_end;
            }
        }
    }

    pub fn get(&self, idx: usize) -> &Option<RenderedLine> {
        static NONE: Option<RenderedLine> = None;
        self.cache.get(idx).unwrap_or(&NONE)
    }
}

/// 向前扩展到 block 边界。
fn expand_to_block_start(lines: &[OutputLine], start: usize) -> usize {
    let mut s = start;
    let mut in_code = false;
    for (_i, line) in lines.iter().enumerate().take(start) {
        let is_md = rendered_lines::is_markdown_style(line.style);
        if is_md && line.content.trim().starts_with("```") {
            in_code = !in_code;
        }
    }

    if in_code {
        let mut scan = start;
        while scan > 0 {
            scan -= 1;
            let line = &lines[scan];
            let is_md = rendered_lines::is_markdown_style(line.style);
            if is_md && line.content.trim().starts_with("```") {
                s = scan;
                break;
            }
        }
    }

    let start_line = &lines[start];
    if rendered_lines::is_markdown_style(start_line.style) {
        let trimmed = start_line.content.trim();
        if super::markdown::is_table_row(trimmed) || super::markdown::is_table_separator(trimmed) {
            let mut scan = start;
            while scan > 0 {
                scan -= 1;
                let prev = &lines[scan];
                let is_md = rendered_lines::is_markdown_style(prev.style);
                let t = prev.content.trim();
                if is_md
                    && (super::markdown::is_table_row(t) || super::markdown::is_table_separator(t))
                {
                    s = scan;
                } else {
                    break;
                }
            }
        }
    }

    s
}

/// 向后扩展到 block 边界。
fn expand_to_block_end(lines: &[OutputLine], end: usize) -> usize {
    let total = lines.len();
    let mut e = end;

    let mut in_code = false;
    for (_i, line) in lines.iter().enumerate().take(end) {
        let is_md = rendered_lines::is_markdown_style(line.style);
        if is_md && line.content.trim().starts_with("```") {
            in_code = !in_code;
        }
    }

    if in_code {
        for i in end..total {
            let line = &lines[i];
            let is_md = rendered_lines::is_markdown_style(line.style);
            if is_md && line.content.trim().starts_with("```") {
                e = i + 1;
                break;
            }
        }
        if e == end {
            e = total;
        }
    }

    if end < total {
        let end_line = &lines[end.min(total) - 1];
        if rendered_lines::is_markdown_style(end_line.style) {
            let trimmed = end_line.content.trim();
            if super::markdown::is_table_row(trimmed)
                || super::markdown::is_table_separator(trimmed)
            {
                for i in end..total {
                    let next = &lines[i];
                    let is_md = rendered_lines::is_markdown_style(next.style);
                    let t = next.content.trim();
                    if is_md
                        && (super::markdown::is_table_row(t)
                            || super::markdown::is_table_separator(t))
                    {
                        e = i + 1;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    e
}

#[cfg(test)]
mod tests {
    use super::super::types::LineStyle;
    use super::*;

    fn md_line(content: &str) -> OutputLine {
        OutputLine {
            content: content.to_string(),
            style: LineStyle::Assistant,
            tool_id: None,
            spans: None,
        }
    }

    #[test]
    fn test_expand_to_block_start_code() {
        let lines = vec![
            md_line("```rust"),
            md_line("fn main() {}"),
            md_line("```"),
            md_line("after"),
        ];
        let s = expand_to_block_start(&lines, 2);
        assert_eq!(s, 0, "should expand to code fence start");
    }

    #[test]
    fn test_expand_to_block_start_table() {
        let lines = vec![
            md_line("before"),
            md_line("| a | b |"),
            md_line("|---|---|"),
            md_line("| 1 | 2 |"),
            md_line("after"),
        ];
        let s = expand_to_block_start(&lines, 3);
        assert_eq!(s, 1, "should expand to table start");
    }

    #[test]
    fn test_expand_to_block_end_code() {
        let lines = vec![
            md_line("```rust"),
            md_line("fn main() {}"),
            md_line("```"),
            md_line("after"),
        ];
        let e = expand_to_block_end(&lines, 1);
        assert_eq!(e, 3, "should expand to code fence end");
    }

    #[test]
    fn test_expand_to_block_end_table() {
        let lines = vec![
            md_line("| a | b |"),
            md_line("|---|---|"),
            md_line("| 1 | 2 |"),
            md_line("after"),
        ];
        let e = expand_to_block_end(&lines, 1);
        assert_eq!(e, 3, "should expand to table end");
    }

    #[test]
    fn test_render_range_basic() {
        let lines = vec![md_line("hello"), md_line("world")];
        let mut cache_inner = vec![None, None];
        rendered_lines::render_range(&lines, 0, 2, 80, &mut cache_inner);
        assert!(cache_inner[0].is_some(), "line 0 should be rendered");
        assert!(cache_inner[1].is_some(), "line 1 should be rendered");
    }
}
