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
    /// #520 workaround（待 ratatui 0.30 升级解除）：上一帧输出的结构签名，
    /// 用于检测结构性变化、触发强制全屏重绘以清除宽字符尾随 cell 残影。
    last_repaint_total: usize,
    last_repaint_blocks: usize,
    last_repaint_scroll: usize,
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
            // usize::MAX 哨兵：保证首帧一定触发一次全屏重绘（清掉进入 alt screen 时的残留）。
            last_repaint_total: usize::MAX,
            last_repaint_blocks: 0,
            last_repaint_scroll: 0,
        }
    }

    /// #520 workaround（待 ratatui 0.30 升级解除）：检测输出是否发生结构性变化，
    /// 需要强制全屏重绘（`terminal.clear()`）以清除 ratatui 0.29 在宽字符尾随 cell
    /// 上遗留的样式残影（手动 resize 能清掉残影即此原理）。
    ///
    /// 触发：首帧、**块数变化**（工具结果 / 新消息等结构块出现或移除）、行数减少
    /// （块内折叠 / 替换）、滚动。
    /// **NEVER** 在流式追加（同一块内行数仅增长、块数不变）时触发，避免逐 token 闪屏。
    /// 终端 resize 不在此处理——ratatui 的 autoresize 会重置 buffer 自行全量重绘。
    /// ratatui 升级到 0.30（已修 trailing-cell diff，PR #2587）后，本 workaround 可整体移除。
    pub fn should_force_repaint(
        &mut self,
        total_lines: usize,
        block_count: usize,
        scroll_pos: usize,
    ) -> bool {
        let changed = total_lines < self.last_repaint_total
            || block_count != self.last_repaint_blocks
            || scroll_pos != self.last_repaint_scroll;
        self.last_repaint_total = total_lines;
        self.last_repaint_blocks = block_count;
        self.last_repaint_scroll = scroll_pos;
        changed
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
    fn test_should_force_repaint_triggers_on_structural_change_not_streaming() {
        // 参数：(total_lines, block_count, scroll_offset)
        let mut area = OutputArea::new();
        // 首帧（哨兵 MAX）：触发，清掉进入 alt screen 的残留。
        assert!(area.should_force_repaint(50, 2, 0), "首帧应触发");
        // 流式追加（同一块内行数增长、块数不变）：不触发，避免逐 token 闪屏。
        assert!(!area.should_force_repaint(60, 2, 0), "块内行数增长不触发");
        assert!(!area.should_force_repaint(80, 2, 0), "继续流式不触发");
        // 工具结果 / 新块出现（块数变化）：触发——即便行数是增长的。
        assert!(area.should_force_repaint(95, 3, 0), "新块出现应触发");
        // 折叠 / 替换（行数减少）：触发。
        assert!(area.should_force_repaint(40, 3, 0), "行数减少应触发");
        // 滚动位置变化：触发。
        assert!(area.should_force_repaint(40, 3, 5), "滚动应触发");
        // 无任何变化：不触发。
        assert!(!area.should_force_repaint(40, 3, 5), "无变化不触发");
    }

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
