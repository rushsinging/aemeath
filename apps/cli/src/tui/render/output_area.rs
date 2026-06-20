use std::collections::HashMap;

use sdk::CharIdx;

use crate::tui::render::output::rendered::RenderedDocument;
use crate::tui::render::output_area::types::DEFAULT_WIDTH;

pub mod content;
pub mod display;
pub mod render;
#[cfg(test)]
mod render_tests;
mod resize;
pub mod selection;
pub mod spinner;
pub mod types;

// 重新导出核心类型，方便外部使用
pub(crate) use render::SCROLLBAR_RESERVE_COLS;
pub use types::{SpanPart, INDENT};

/// 可滚动输出区域，显示对话历史
pub struct OutputArea {
    pub term_width: usize,
    /// 屏幕行到逻辑行的映射：每项是 (逻辑行索引, chunk内的char起始偏移, chunk内的char结束偏移)
    pub screen_line_map: Vec<(usize, CharIdx, CharIdx)>,
    /// 渲染后的逻辑行文本覆盖
    pub rendered_line_content: HashMap<usize, String>,
    /// todo id -> subject 缓存
    pub todo_subject_cache: std::collections::HashMap<String, String>,
    /// 新输出渲染管线产物（spans + plain）。
    pub document: RenderedDocument,
}

impl Default for OutputArea {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputArea {
    pub fn new() -> Self {
        let term_width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(DEFAULT_WIDTH)
            .saturating_sub(2);

        Self {
            term_width,
            screen_line_map: Vec::new(),
            rendered_line_content: HashMap::new(),
            todo_subject_cache: std::collections::HashMap::new(),
            document: RenderedDocument::default(),
        }
    }

    pub(crate) fn replace_document(&mut self, document: RenderedDocument) {
        self.document = document;
    }

    pub fn document(&self) -> &RenderedDocument {
        &self.document
    }

    /// 测试辅助：以 `count` 行纯文本填充 document（单 block）。
    #[cfg(test)]
    pub(crate) fn set_plain_document_lines(&mut self, count: usize) {
        use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};
        use ratatui::text::Span;
        use std::rc::Rc;

        let lines: Vec<RenderedLine> = (0..count)
            .map(|i| RenderedLine::new(vec![Span::raw(format!("line {i}"))]))
            .collect();
        self.document = RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "test".into(),
                lines: Rc::new(lines),
            }],
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};
    use ratatui::text::Span;
    use std::rc::Rc;

    #[test]
    fn test_output_area_replace_document_replaces_content() {
        let mut area = OutputArea::new();
        let document = RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "a".into(),
                lines: Rc::new(vec![RenderedLine::new(vec![Span::raw("x")])]),
            }],
        };
        area.replace_document(document);

        assert_eq!(area.document().total_lines(), 1);
    }
}
