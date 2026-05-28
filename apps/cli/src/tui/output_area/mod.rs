use std::collections::{HashMap, VecDeque};

use sdk::CharIdx;

use ratatui::{buffer::Buffer, layout::Rect};

use crate::tui::output_area::types::DEFAULT_WIDTH;

pub mod content;
pub mod display;
mod queued;
pub mod render;
mod resize;
pub mod scroll;
pub mod selection;
mod selection_render;
pub mod spinner;
pub mod streaming;
pub mod types;

#[cfg(test)]
mod content_tests;

// 重新导出核心类型，方便外部使用
pub use crate::tui::render::output::diff::build_diff_lines;
pub use crate::tui::render::output::markdown;
pub use crate::tui::render::output::tool_display;
pub use types::{LineStyle, OutputLine, SpanPart, SpinnerState, INDENT, MAX_LINES};

use crate::tui::view_state::cache::OutputRenderCacheState;

/// 可滚动输出区域，显示对话历史
pub struct OutputArea {
    pub lines: VecDeque<OutputLine>,
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub last_line_count: usize,
    pub term_width: usize,
    /// 当前流式助手块的完整文本
    pub streaming_buffer: String,
    /// lines 中当前流式块的起始索引
    pub streaming_start: Option<usize>,
    /// 是否为合成的未闭合 think 标签
    pub synthetic_think_open: bool,
    /// 排队的用户消息行数（流式过程中添加的）
    pub queued_line_count: usize,
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
    /// 排队的用户消息
    pub queued_messages: Vec<String>,
    /// AskUserQuestion 互动块在 lines 中的起始索引，用于提交后折叠
    pub ask_user_block_start: Option<usize>,
    /// 渲染缓存（滑动窗口）
    pub(crate) rendered_cache: OutputRenderCacheState,
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
            streaming_buffer: String::new(),
            streaming_start: None,
            synthetic_think_open: false,
            queued_line_count: 0,
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            screen_line_map: Vec::new(),
            rendered_line_content: HashMap::new(),
            spinner: None,
            last_visible_height: 0,
            todo_subject_cache: std::collections::HashMap::new(),
            task_status_lines: Vec::new(),
            queued_messages: Vec::new(),
            ask_user_block_start: None,
            rendered_cache: OutputRenderCacheState::default(),
        }
    }

    /// 显示欢迎横幅。
    pub fn init(&mut self) {
        self.push_system("Aemeath - AI Agent");
        self.push_system("");
        self.push_system("Type /help for available commands");
        self.push_system("");
    }

    /// 绘制输出区域。
    #[allow(dead_code)]
    pub fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        self.render(area, buf);
    }
}
