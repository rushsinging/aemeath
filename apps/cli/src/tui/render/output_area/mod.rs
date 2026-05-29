use std::collections::{HashMap, VecDeque};

use sdk::CharIdx;

use crate::tui::render::output::document_renderer::OutputDocumentRenderer;
use crate::tui::render::output::rendered::RenderedDocument;
use crate::tui::render::output_area::types::DEFAULT_WIDTH;

pub mod content;
pub mod display;
pub mod render;
mod resize;
pub mod scroll;
pub mod selection;
mod selection_render;
pub mod spinner;
pub mod types;

#[cfg(test)]
mod content_tests;

// 重新导出核心类型，方便外部使用
pub use types::{LineStyle, OutputLine, SpanPart, SpinnerState, INDENT, MAX_LINES};

/// 可滚动输出区域，显示对话历史
pub struct OutputArea {
    pub lines: VecDeque<OutputLine>,
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub last_line_count: usize,
    pub term_width: usize,
    /// 鼠标是否正在拖拽选择
    pub is_selecting: bool,
    /// 选择起始点：(逻辑行索引, char 偏移)
    pub selection_start: Option<(usize, CharIdx)>,
    /// 选择结束点：(逻辑行索引, char 偏移)
    pub selection_end: Option<(usize, CharIdx)>,
    /// 屏幕行到逻辑行的映射：每项是 (逻辑行索引, chunk内的char起始偏移, chunk内的char结束偏移)
    pub screen_line_map: Vec<(usize, CharIdx, CharIdx)>,
    /// 渲染后的逻辑行文本覆盖
    pub rendered_line_content: HashMap<usize, String>,
    /// 活跃的 spinner 动画
    pub spinner: Option<SpinnerState>,
    /// 上次渲染时的可见高度缓存
    pub last_visible_height: usize,
    /// todo id -> subject 缓存
    pub todo_subject_cache: std::collections::HashMap<String, String>,
    /// spinner 下方显示的任务状态行
    pub task_status_lines: Vec<String>,
    /// AskUserQuestion 互动块在 lines 中的起始索引，用于提交后折叠
    pub ask_user_block_start: Option<usize>,
    /// 新输出渲染管线产物（spans + plain）。
    pub document: RenderedDocument,
    /// 新输出渲染管线的 block 级缓存渲染器。
    pub document_renderer: OutputDocumentRenderer,
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
            lines: VecDeque::with_capacity(MAX_LINES),
            scroll_offset: 0,
            auto_scroll: true,
            last_line_count: 0,
            term_width,
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            screen_line_map: Vec::new(),
            rendered_line_content: HashMap::new(),
            spinner: None,
            last_visible_height: 0,
            todo_subject_cache: std::collections::HashMap::new(),
            task_status_lines: Vec::new(),
            ask_user_block_start: None,
            document: RenderedDocument::default(),
            document_renderer: OutputDocumentRenderer::default(),
        }
    }

    pub fn set_document(&mut self, document: RenderedDocument) {
        self.document = document;
    }

    pub fn document(&self) -> &RenderedDocument {
        &self.document
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};
    use ratatui::text::Span;

    #[test]
    fn test_output_area_set_document_replaces_content() {
        let mut area = OutputArea::new();
        let document = RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "a".into(),
                lines: vec![RenderedLine::new(vec![Span::raw("x")])],
            }],
        };
        area.set_document(document);

        assert_eq!(area.document().total_lines(), 1);
    }
}
