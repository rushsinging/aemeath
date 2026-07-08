mod diagnostics;
mod paint;
mod scrollbar;
mod selection;

use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};
use sdk::CharIdx;

use super::OutputArea;
use crate::tui::render::display::safe_text::str_display_width;
use crate::tui::render::output::selection_overlay::apply_selection_overlay;
use crate::tui::view_model::LiveStatusViewModel;
use crate::tui::view_state::output::OutputViewState;
use diagnostics::{diagnostic_plain, normalize_rendered_table_plain};
use paint::{clear_area, paint_line_fill_styles, trim_line_fill_styles};
use selection::sel_range_for_line;

pub(crate) use scrollbar::{content_area_for_scrollbar, SCROLLBAR_RESERVE_COLS};
use scrollbar::{render_scrollbar, visible_range};
pub(crate) use selection::sel_range_for_bounds;

impl OutputArea {
    /// 渲染输出区域
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut ratatui::buffer::Buffer,
        view: &OutputViewState,
        live_status: &LiveStatusViewModel,
    ) {
        if area.height == 0 {
            return;
        }

        let new_width = (area.width as usize).saturating_sub(2);
        if new_width != self.term_width {
            self.term_width = new_width;
        }

        let spinner_line = live_status
            .spinner
            .as_ref()
            .map(|spinner| self.build_spinner_line(spinner, live_status.compact_progress.as_ref()));

        let visible_lines = view.last_visible_height;
        let total_lines = self.document.total_lines();
        let needs_scrollbar = total_lines > visible_lines;
        let content_area = content_area_for_scrollbar(area, needs_scrollbar);
        let (start, end) = visible_range(
            total_lines,
            visible_lines,
            view.auto_scroll,
            view.scroll_offset,
        );

        clear_area(area, buf);

        let document_lines = self.document.iter_lines().collect::<Vec<_>>();
        let first_visible_doc_line_plain = document_lines
            .get(start)
            .map(|line| diagnostic_plain(&line.plain))
            .unwrap_or_default();
        let last_visible_doc_line_plain = end
            .checked_sub(1)
            .and_then(|idx| document_lines.get(idx))
            .map(|line| diagnostic_plain(&line.plain))
            .unwrap_or_default();
        let last_doc_line_plain = document_lines
            .last()
            .map(|line| diagnostic_plain(&line.plain))
            .unwrap_or_default();
        let visible_overwide_lines = document_lines
            .get(start..end)
            .unwrap_or(&[])
            .iter()
            .filter(|line| str_display_width(&line.plain) > content_area.width as usize)
            .count();
        let max_visible_line_width = document_lines
            .get(start..end)
            .unwrap_or(&[])
            .iter()
            .map(|line| str_display_width(&line.plain))
            .max()
            .unwrap_or(0);
        let mut screen_map = Vec::new();
        let mut rendered_content = std::collections::HashMap::new();
        let mut display_lines = Vec::new();
        let mut line_fill_styles = Vec::new();

        for idx in start..end {
            let Some(line) = document_lines.get(idx) else {
                continue;
            };
            let mut plain = line.plain.clone();
            if idx == start && plain.contains('│') {
                plain = normalize_rendered_table_plain(&plain);
            }
            // #520 诊断日志：对表格行（含 │）记录显示宽度与渲染区域宽度的对比
            if plain.contains('│') {
                let line_disp_w = str_display_width(&plain);
                crate::tui::log_trace!(
                    "tui.output.table_line idx={} line_display_width={} content_area_width={} overflow={} plain={:?}",
                    idx,
                    line_disp_w,
                    content_area.width,
                    line_disp_w > content_area.width as usize,
                    &plain,
                );
            }
            let char_end = CharIdx::new(plain.chars().count());
            screen_map.push((idx, CharIdx::ZERO, char_end));
            rendered_content.insert(idx, plain);
            let spans = apply_selection_overlay(line, sel_range_for_line(view, line, idx));
            line_fill_styles.push(line.fill_style);
            display_lines.push(Line::from(spans).style(line.style));
        }

        self.screen_line_map = screen_map;
        self.rendered_line_content = rendered_content;
        let before_status_lines = display_lines.len();
        self.append_status_lines(&mut display_lines, &spinner_line, live_status, view);
        let after_status_lines = display_lines.len();
        line_fill_styles.resize(display_lines.len(), None);
        let line_fill_styles = trim_line_fill_styles(line_fill_styles, area.height as usize);
        let display_lines = self.trim_to_area_height(display_lines, area.height as usize);
        crate::tui::log_trace!(
            "tui.output.render area={}x{} content={}x{} total_lines={} visible_lines={} auto_scroll={} scroll_offset={} range={}..{} needs_scrollbar={} spinner={} queued_lines={} task_lines={} before_status_lines={} after_status_lines={} after_trim_lines={} visible_overwide_lines={} max_visible_line_width={} first_visible={:?} last_visible={:?} last_doc={:?}",
            area.width,
            area.height,
            content_area.width,
            content_area.height,
            total_lines,
            visible_lines,
            view.auto_scroll,
            view.scroll_offset,
            start,
            end,
            needs_scrollbar,
            spinner_line.is_some(),
            live_status.queued_lines.len(),
            live_status.task_lines.len(),
            before_status_lines,
            after_status_lines,
            display_lines.len(),
            visible_overwide_lines,
            max_visible_line_width,
            first_visible_doc_line_plain,
            last_visible_doc_line_plain,
            last_doc_line_plain
        );

        // 根因修复（#196）：paragraph.render 复用 ratatui buffer cell 数组，
        // 上一帧长行 → 本帧短行时尾部 cell 不会被清，导致"行重叠 / 残影"。
        // 在 paint 背景与 paragraph 渲染之前先把 content_area 整片 reset。
        clear_area(content_area, buf);
        paint_line_fill_styles(content_area, buf, &line_fill_styles);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paragraph = Paragraph::new(display_lines);
            paragraph.render(content_area, buf);
        }));

        render_scrollbar(
            area,
            buf,
            total_lines,
            visible_lines,
            view.auto_scroll,
            view.scroll_offset,
        );
    }
}
