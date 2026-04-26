use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};
use std::collections::VecDeque;

use aemeath_core::string_idx::CharIdx;

use crate::tui::output_area::display::wrap_line;
use crate::tui::output_area::types::DEFAULT_WIDTH;

pub mod types;
pub mod diff;
pub mod streaming;
pub mod content;
pub mod scroll;
pub mod markdown;
pub mod spinner;
pub mod display;
pub mod selection;
pub mod tool_display;

// 重新导出核心类型，方便外部使用
pub use types::{OutputLine, LineStyle, SpinnerState, MAX_LINES, INDENT};
pub use diff::build_diff_lines;

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
    /// 选择起始点：(屏幕行索引, char 偏移)
    pub selection_start: Option<(usize, CharIdx)>,
    /// 选择结束点：(屏幕行索引, char 偏移)
    pub selection_end: Option<(usize, CharIdx)>,
    /// 屏幕行到逻辑行的映射：每项是 (逻辑行索引, chunk内的char起始偏移, chunk内的char结束偏移)
    /// 由 render() 构建，供 selection 使用
    pub screen_line_map: Vec<(usize, CharIdx, CharIdx)>,
    /// 活跃的 spinner 动画（显示为最后一行）
    pub spinner: Option<SpinnerState>,
    /// 上次渲染时的可见高度缓存
    pub last_visible_height: usize,
    /// todo id -> subject 缓存
    pub todo_subject_cache: std::collections::HashMap<String, String>,
    /// spinner 下方显示的任务状态行（外部更新）
    pub task_status_lines: Vec<String>,
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
            spinner: None,
            last_visible_height: 0,
            todo_subject_cache: std::collections::HashMap::new(),
            task_status_lines: Vec::new(),
        }
    }

    /// 渲染输出区域
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        // 推进 spinner 帧
        if let Some(ref mut s) = self.spinner {
            s.frame = s.frame.wrapping_add(1);
        }

        // 更新宽度
        self.term_width = (area.width as usize).saturating_sub(2);

        // 构建 spinner 行和任务状态行
        let spinner_line = self.build_spinner_line();
        let task_line_count = if self.spinner.is_some() { self.task_status_lines.len() } else { 0 };
        let reserved = if spinner_line.is_some() { 1 + task_line_count } else { 0 };

        let visible_lines = (area.height as usize).saturating_sub(reserved);
        self.last_visible_height = visible_lines;
        let total_lines = self.lines.len();

        let (start, end) = if self.auto_scroll {
            let start = total_lines.saturating_sub(visible_lines);
            (start, total_lines)
        } else {
            let max_start = total_lines.saturating_sub(visible_lines);
            let start = max_start.saturating_sub(self.scroll_offset);
            let start = start.min(max_start);
            (start, (start + visible_lines).min(total_lines))
        };

        // 清除区域
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].reset();
            }
        }

        let spinner_frame_idx = self.spinner.as_ref().map(|s| s.frame).unwrap_or(0);

        // 构建 screen_line_map：记录每个屏幕行对应的 (逻辑行索引, char起始, char结束)
        let mut new_screen_map = Vec::new();

        // 预扫描 fenced code blocks（只对 assistant/thinking/system 内容生效）
        let mut in_code_block = false;
        let mut code_block_lines = std::collections::HashSet::new();
        for (i, line) in self.lines.iter().enumerate().skip(start).take(end - start) {
            let is_markdown_style = matches!(line.style, LineStyle::Assistant | LineStyle::Thinking | LineStyle::System);
            if is_markdown_style && line.content.trim().starts_with("```") {
                in_code_block = !in_code_block;
                code_block_lines.insert(i);
            } else if in_code_block && is_markdown_style {
                code_block_lines.insert(i);
            }
        }
        // 如果代码块未闭合，包含 fence 行
        let code_style = Style::default()
            .bg(Color::Rgb(40, 44, 52))
            .fg(Color::Rgb(171, 178, 191));

        // 预扫描 markdown 表格块（连续的 | ... | 行）
        // table_block_lines: 属于某个表格块的所有逻辑行索引
        // table_block_info: 每个表格块的起始索引和行内容引用索引列表
        let mut table_block_lines = std::collections::HashSet::new();
        {
            let visible: Vec<(usize, &OutputLine)> = self.lines.iter()
                .enumerate()
                .skip(start)
                .take(end - start)
                .collect();
            let mut i = 0;
            while i < visible.len() {
                let (_, line) = visible[i];
                let is_md = matches!(line.style, LineStyle::Assistant | LineStyle::Thinking | LineStyle::System);
                if is_md && markdown::is_table_row(&line.content) {
                    // 找到连续表格行
                    let block_start = i;
                    let mut block_end = i + 1;
                    while block_end < visible.len() {
                        let (_, next_line) = visible[block_end];
                        let next_is_md = matches!(next_line.style, LineStyle::Assistant | LineStyle::Thinking | LineStyle::System);
                        let trimmed = next_line.content.trim();
                        if next_is_md && (markdown::is_table_row(trimmed) || markdown::is_table_separator(trimmed)) {
                            block_end += 1;
                        } else {
                            break;
                        }
                    }
                    // 至少要有 separator 行才算合法表格
                    let has_separator = (block_start..block_end).any(|j| markdown::is_table_separator(visible[j].1.content.trim()));
                    if has_separator {
                        for j in block_start..block_end {
                            table_block_lines.insert(visible[j].0);
                        }
                    }
                    i = block_end;
                } else {
                    i += 1;
                }
            }
        }

        // 预渲染表格块：table_block_first_idx -> 渲染结果
        let mut table_render_cache: std::collections::HashMap<usize, Vec<Vec<Span<'static>>>> = std::collections::HashMap::new();
        {
            let visible: Vec<(usize, &OutputLine)> = self.lines.iter()
                .enumerate()
                .skip(start)
                .take(end - start)
                .collect();
            let mut i = 0;
            while i < visible.len() {
                let (idx, _) = visible[i];
                if table_block_lines.contains(&idx) {
                    let block_start = i;
                    let mut block_end = i + 1;
                    while block_end < visible.len() && table_block_lines.contains(&visible[block_end].0) {
                        block_end += 1;
                    }
                    // 收集块内容
                    let block_lines: Vec<&str> = (block_start..block_end)
                        .map(|j| visible[j].1.content.as_str())
                        .collect();
                    let base = visible[block_start].1.style.to_style();
                    let rendered = markdown::render_table_block(&block_lines, base);
                    table_render_cache.insert(visible[block_start].0, rendered);
                    i = block_end;
                } else {
                    i += 1;
                }
            }
        }

        // 用显式迭代替代 .map().flatten()，以支持表格块等多行组渲染
        let vis_lines: Vec<(usize, &OutputLine)> = self.lines
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
            .collect();

        let mut lines: Vec<Line> = Vec::new();
        let mut vi = 0;
        while vi < vis_lines.len() {
            let (idx, output_line) = vis_lines[vi];
            let style = output_line.style;

            // 检查是否为表格块起始行
            if let Some(table_rows) = table_render_cache.get(&idx) {
                // 渲染整个表格块
                for row_spans in table_rows {
                    // 每个表格行的 spans 合成一个屏幕行（不 wrap，表格行不应换行）
                    let line_text: String = row_spans.iter().map(|s| s.content.clone().into_owned()).collect();
                    let sanitized = display::sanitize_for_display(&line_text);
                    let char_offsets = compute_char_offsets(&sanitized, self.term_width);
                    let wrapped = wrap_line(&line_text, self.term_width);
                    for (chunk_idx, _) in wrapped.iter().enumerate() {
                        let (char_start, char_end) = if chunk_idx < char_offsets.len() {
                            char_offsets[chunk_idx]
                        } else {
                            (CharIdx::ZERO, CharIdx::ZERO)
                        };
                        new_screen_map.push((idx, char_start, char_end));
                    }
                    // 渲染每个 wrap chunk
                    if self.has_real_selection() {
                        let screen_start = new_screen_map.len() - wrapped.len();
                        for (chunk_idx, chunk) in wrapped.into_iter().enumerate() {
                            let screen_idx = screen_start + chunk_idx;
                            let base_s = style.to_style();
                            let line_spans = self.render_line_with_selection(screen_idx, &chunk, base_s, &new_screen_map);
                            lines.push(Line::from(line_spans));
                        }
                    } else {
                        // 无选择：如果只有 1 个 wrapped 行，直接用 table spans
                        if wrapped.len() == 1 {
                            lines.push(Line::from(row_spans.clone()));
                        } else {
                            // wrap 后每行用 plain text（表格不太可能 wrap，但保底）
                            for chunk in wrapped {
                                lines.push(Line::styled(chunk, style.to_style()));
                            }
                        }
                    }
                }
                // 跳过整个表格块
                vi += table_rows.len();
                continue;
            }

            // 非表格行：原逻辑
            let wrapped = wrap_line(&output_line.content, self.term_width);

            let sanitized = display::sanitize_for_display(&output_line.content);
            let char_offsets = compute_char_offsets(&sanitized, self.term_width);
            for (chunk_idx, _) in wrapped.iter().enumerate() {
                let (char_start, char_end) = if chunk_idx < char_offsets.len() {
                    char_offsets[chunk_idx]
                } else {
                    (CharIdx::ZERO, CharIdx::ZERO)
                };
                new_screen_map.push((idx, char_start, char_end));
            }

            let is_markdown = matches!(style, LineStyle::Assistant | LineStyle::Thinking | LineStyle::System);
            let is_code_block = code_block_lines.contains(&idx);
            let has_real_selection = self.has_real_selection();

            let rendered: Vec<Line> = if has_real_selection {
                let screen_start = new_screen_map.len() - wrapped.len();
                wrapped.into_iter().enumerate().map(|(chunk_idx, chunk)| {
                    let screen_idx = screen_start + chunk_idx;
                    if is_code_block {
                        let line_spans = self.render_line_with_selection(screen_idx, &chunk, code_style, &new_screen_map);
                        Line::from(line_spans)
                    } else if is_markdown {
                        let md_spans = markdown::inline_markdown_spans(&chunk, style.to_style());
                        let line_spans = self.render_spans_with_selection(screen_idx, &md_spans, &new_screen_map);
                        Line::from(line_spans)
                    } else {
                        let line_spans = self.render_line_with_selection(screen_idx, &chunk, style.to_style(), &new_screen_map);
                        Line::from(line_spans)
                    }
                }).collect()
            } else {
                wrapped.into_iter().map(|chunk| {
                    if is_code_block {
                        Line::styled(chunk, code_style)
                    } else if is_markdown {
                        Line::from(markdown::inline_markdown_spans(&chunk, style.to_style()))
                    } else {
                        Line::styled(chunk, style.to_style())
                    }
                }).collect()
            };
            lines.extend(rendered);
            vi += 1;
        }

        self.screen_line_map = new_screen_map;

        // 追加 spinner 和任务状态行
        if let Some(sl) = spinner_line {
            lines.push(sl);
            for task_line in &self.task_status_lines {
                lines.push(Line::styled(
                    format!("  {task_line}"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

        let lines: Vec<Line> = if lines.len() > area.height as usize {
            let offset = lines.len() - area.height as usize;
            log::debug!(
                "trim: lines.len={}, area.height={}, offset={}, screen_map.len={}",
                lines.len(), area.height, offset, self.screen_line_map.len()
            );
            self.screen_line_map = self.screen_line_map.split_off(offset);
            lines.into_iter().skip(offset).collect()
        } else {
            lines
        };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paragraph = Paragraph::new(lines);
            paragraph.render(area, buf);
        }));

        let total_rendered = self.screen_line_map.len();
        if total_rendered > 0 {
            log::debug!(
                "render: screen_map after trim: first=[{:?}], last=[{:?}], total={}",
                self.screen_line_map.first(),
                self.screen_line_map.last(),
                total_rendered,
            );
        }

        // 后处理：tool call 行的 dot 颜色
        // 遍历 self.lines 中可见范围内的 tool call 行，修改 buf 上 dot 字符的颜色
        {
            let blink_on = (spinner_frame_idx / 10) % 2 == 0;
            for (si, &(li, _, _)) in self.screen_line_map.iter().enumerate() {
                if li >= self.lines.len() { continue; }
                let line = &self.lines[li];
                let content = &line.content;
                // 计算屏幕 y 坐标
                let visible_offset = total_rendered.saturating_sub(area.height as usize);
                let screen_y = si.saturating_sub(visible_offset);
                if screen_y >= area.height as usize { continue; }
                let buf_y = area.y + screen_y as u16;

                let dot_color = match line.style {
                    LineStyle::ToolCallRunning if content.starts_with('●') => {
                        Some(if blink_on { Color::White } else { Color::DarkGray })
                    }
                    LineStyle::ToolCallSuccess if content.starts_with('✓') => Some(Color::Green),
                    LineStyle::ToolCallError if content.starts_with('✗') => Some(Color::Red),
                    _ => None,
                };

                if let Some(color) = dot_color {
                    // 修改第一个字符（dot）的颜色
                    if let Some(cell) = buf.cell_mut((area.x, buf_y)) {
                        let _ch = cell.symbol().to_string();
                        cell.set_char('●');
                        let mut s = cell.style();
                        s.fg = Some(color);
                        cell.set_style(s);
                    }
                }
            }
        }

        // 渲染滚动条
        if total_lines > visible_lines {
            let scrollbar_area = Rect {
                x: area.right().saturating_sub(1),
                y: area.top(),
                width: 1,
                height: area.height,
            };
            let max_scroll = total_lines.saturating_sub(visible_lines);
            let current_position = if self.auto_scroll {
                max_scroll
            } else {
                max_scroll.saturating_sub(self.scroll_offset)
            };
            let mut scrollbar_state = ScrollbarState::new(max_scroll).position(current_position);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            StatefulWidget::render(scrollbar, scrollbar_area, buf, &mut scrollbar_state);
        }

        self.last_line_count = total_lines;
    }

    /// 渲染带选择高亮的单行（screen_idx 是屏幕行索引）
    fn render_line_with_selection(&self, screen_idx: usize, content: &str, base_style: Style, screen_map: &[(usize, CharIdx, CharIdx)]) -> Vec<Span<'static>> {
        let Some((start_screen, start_col)) = self.selection_start else {
            return vec![Span::styled(content.to_string(), base_style)];
        };
        let Some((end_screen, end_col)) = self.selection_end else {
            return vec![Span::styled(content.to_string(), base_style)];
        };

        // 归一化：确保 start <= end
        let (start_screen, start_col, end_screen, end_col) = if start_screen < end_screen
            || (start_screen == end_screen && start_col < end_col)
        {
            (start_screen, start_col, end_screen, end_col)
        } else {
            (end_screen, end_col, start_screen, start_col)
        };

        let selection_style = Style::default().bg(Color::Blue).fg(Color::White);

        // 当前屏幕行不在选中范围内
        if screen_idx < start_screen || screen_idx > end_screen {
            return vec![Span::styled(content.to_string(), base_style)];
        }
        // 起止相同但没实际选中
        if start_screen == end_screen && start_col == end_col {
            return vec![Span::styled(content.to_string(), base_style)];
        }

        let chars: Vec<char> = content.chars().collect();
        let chars_len = chars.len();

        // 计算本行的选中起止列（转为 usize 以索引 chars vec）
        let chunk_start = if screen_idx < screen_map.len() {
            screen_map[screen_idx].1
        } else {
            CharIdx::ZERO
        };
        let line_start: usize = if screen_idx == start_screen {
            start_col.saturating_sub(chunk_start)
        } else {
            0
        };
        let line_end: usize = if screen_idx == end_screen {
            end_col.saturating_sub(chunk_start).min(chars_len)
        } else {
            chars_len
        };

        let mut spans = Vec::new();
        let mut current_text = String::new();
        let mut current_is_selected = false;

        for (i, &ch) in chars.iter().enumerate() {
            let is_selected = i >= line_start && i < line_end;
            if is_selected != current_is_selected && !current_text.is_empty() {
                let style = if current_is_selected { selection_style } else { base_style };
                spans.push(Span::styled(std::mem::take(&mut current_text), style));
            }
            current_text.push(ch);
            current_is_selected = is_selected;
        }

        if !current_text.is_empty() {
            let style = if current_is_selected { selection_style } else { base_style };
            spans.push(Span::styled(current_text, style));
        }

        if spans.is_empty() {
            spans.push(Span::styled(content.to_string(), base_style));
        }

        spans
    }

    /// 是否有实际选中范围（start != end）
    fn has_real_selection(&self) -> bool {
        match (self.selection_start, self.selection_end) {
            (Some((ss, sc)), Some((es, ec))) => ss != es || sc != ec,
            _ => false,
        }
    }

    /// 对已有的 markdown spans 叠加选中高亮
    /// 不在选中范围内的 span 保持原样，选中部分改为 selection style
    fn render_spans_with_selection(&self, screen_idx: usize, spans: &[Span<'static>], screen_map: &[(usize, CharIdx, CharIdx)]) -> Vec<Span<'static>> {
        let Some((start_screen, start_col)) = self.selection_start else {
            return spans.to_vec();
        };
        let Some((end_screen, end_col)) = self.selection_end else {
            return spans.to_vec();
        };

        let (start_screen, start_col, end_screen, end_col) = if start_screen < end_screen
            || (start_screen == end_screen && start_col < end_col)
        {
            (start_screen, start_col, end_screen, end_col)
        } else {
            (end_screen, end_col, start_screen, start_col)
        };

        if start_screen == end_screen && start_col == end_col {
            return spans.to_vec();
        }
        if screen_idx < start_screen || screen_idx > end_screen {
            return spans.to_vec();
        }

        let selection_style = Style::default().bg(Color::Blue).fg(Color::White);

        // 展开所有 span 的字符，记录每个字符的原始 style
        let mut all_chars: Vec<(char, Style)> = Vec::new();
        for span in spans {
            for ch in span.content.chars() {
                all_chars.push((ch, span.style));
            }
        }

        // 计算本行在展开字符流中的选中范围
        let chunk_start = if screen_idx < screen_map.len() {
            screen_map[screen_idx].1
        } else {
            CharIdx::ZERO
        };
        let line_start: usize = if screen_idx == start_screen {
            start_col.saturating_sub(chunk_start)
        } else {
            0
        };
        let line_end: usize = if screen_idx == end_screen {
            end_col.saturating_sub(chunk_start).min(all_chars.len())
        } else {
            all_chars.len()
        };

        // 重新构建 spans
        let mut result = Vec::new();
        let mut current_text = String::new();
        let mut current_style: Option<Style> = None;

        for (i, (ch, base_style)) in all_chars.iter().enumerate() {
            let is_selected = i >= line_start && i < line_end;
            let style = if is_selected { selection_style } else { *base_style };

            if current_style != Some(style) {
                if !current_text.is_empty() {
                    result.push(Span::styled(
                        std::mem::take(&mut current_text),
                        current_style.unwrap_or(*base_style),
                    ));
                }
                current_style = Some(style);
            }
            current_text.push(*ch);
        }

        if !current_text.is_empty() {
            result.push(Span::styled(current_text, current_style.unwrap()));
        }

        result
    }
}
  
/// 计算 wrap 后每个 chunk 在原始文本中的 char 偏移 (start, end)
fn compute_char_offsets(text: &str, max_width: usize) -> Vec<(CharIdx, CharIdx)> {
    use unicode_width::UnicodeWidthChar;
    if max_width == 0 {
        let len = text.chars().count();
        return vec![(CharIdx::ZERO, CharIdx::new(len))];
    }

    let mut result = Vec::new();
    let mut current_width = 0usize;
    let mut chunk_start = 0usize; // char count

    for (char_idx, ch) in text.chars().enumerate() {
        let ch_width = ch.width().unwrap_or(1) as usize;
        if current_width + ch_width > max_width {
            result.push((CharIdx::new(chunk_start), CharIdx::new(char_idx)));
            chunk_start = char_idx;
            current_width = 0;
        }
        current_width += ch_width;
    }

    let end = text.chars().count();
    result.push((CharIdx::new(chunk_start), CharIdx::new(end)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todowrite_real_input_from_session() {
        let raw = r#"{"todos":[{"activeForm":"Reviewing aemeath-core","description":"Read","id":"1","status":"in_progress","subject":"Review aemeath-core (核心逻辑)"},{"activeForm":"Reviewing aemeath-llm","description":"Read","id":"2","status":"pending","subject":"Review aemeath-llm (LLM 抽象层)"},{"activeForm":"Reviewing aemeath-tools","description":"Read","id":"3","status":"pending","subject":"Review aemeath-tools (工具实现)"}]}"#;
        let (header, details) = tool_display::format_tool_call("TodoWrite", raw);
        println!("HEADER: {header}");
        for d in &details {
            println!("DETAIL: {d}");
        }
        assert!(header.contains("3 items"), "header was: {header}");
        assert!(details[0].contains("核心"), "detail[0]: {}", details[0]);
        assert!(details[0].starts_with("◐"), "detail[0] icon: {}", details[0]);
        assert!(details[1].starts_with("○"), "detail[1] icon: {}", details[1]);
    }

    #[test]
    fn todowrite_via_value_to_string_roundtrip() {
        let v: serde_json::Value = serde_json::from_str(r#"{"todos":[{"subject":"Review aemeath-core (核心逻辑)","status":"in_progress"},{"subject":"T2","status":"pending"}]}"#).unwrap();
        let s = v.to_string();
        println!("ROUNDTRIP STRING: {s}");
        let (header, details) = tool_display::format_tool_call("TodoWrite", &s);
        println!("HEADER: {header}");
        for d in &details {
            println!("DETAIL: {d}");
        }
        assert!(details[0].contains("核心"), "detail[0]: {}", details[0]);
        assert!(details[0].starts_with("◐"));
    }

    #[test]
    fn todorun_with_max_turns() {
        let raw = r#"{"max_turns_per_todo": 100}"#;
        let (header, details) = tool_display::format_tool_call("TodoRun", raw);
        assert_eq!(header, "● TodoRun");
        assert_eq!(details.len(), 1);
        assert_eq!(details[0], "execute all pending todos");
    }

    #[test]
    fn todorun_without_max_turns() {
        let raw = "{}";
        let (header, details) = tool_display::format_tool_call("TodoRun", raw);
        assert_eq!(header, "● TodoRun");
        assert_eq!(details.len(), 1);
        assert_eq!(details[0], "execute all pending todos");
    }
}
