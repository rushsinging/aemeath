use std::collections::{HashMap, VecDeque};

use aemeath_core::string_idx::CharIdx;

use crate::tui::output_area::types::DEFAULT_WIDTH;

pub mod content;
pub mod diff;
pub mod display;
pub mod markdown;
mod queued;
mod render;
mod render_blocks;
mod render_status;
pub mod scroll;
pub mod selection;
mod selection_render;
pub mod spinner;
pub mod streaming;
pub mod tool_display;
pub mod types;

#[cfg(test)]
mod content_tests;
#[cfg(test)]
mod render_blocks_tests;

// 重新导出核心类型，方便外部使用
pub use diff::build_diff_lines;
pub use types::{LineStyle, OutputLine, SpanPart, SpinnerState, INDENT, MAX_LINES};

/// 可滚动的输出区域，显示对话历史
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
    /// 由 render() 构建，供 selection 使用
    pub screen_line_map: Vec<(usize, CharIdx, CharIdx)>,
    /// 渲染后的逻辑行文本覆盖：Markdown 表格等渲染文本与原始 content 不同时，selection 使用这里的数据源
    pub rendered_line_content: HashMap<usize, String>,
    /// 活跃的 spinner 动画（显示为最后一行）
    pub spinner: Option<SpinnerState>,
    /// 上次渲染时的可见高度缓存
    pub last_visible_height: usize,
    /// todo id -> subject 缓存
    pub todo_subject_cache: std::collections::HashMap<String, String>,
    /// spinner 下方显示的任务状态行（外部更新）
    pub task_status_lines: Vec<String>,
    /// 排队的用户消息（显示在 spinner 上方）
    pub queued_messages: Vec<String>,
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
        }
    }
}
