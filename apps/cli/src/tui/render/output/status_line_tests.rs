use ratatui::{buffer::Buffer, layout::Rect};

use super::*;
use crate::tui::render::output_area::selection::output_selection_view_for_test;
use crate::tui::view_model::{LiveStatusViewModel, SpinnerLineView};

fn live_status(task_lines: Vec<&str>) -> LiveStatusViewModel {
    LiveStatusViewModel {
        spinner: Some(SpinnerLineView {
            frame: 0,
            verb: "Thinking".to_string(),
            elapsed_secs: 0,
            phase_elapsed_secs: Some(0),
            phase_text: None,
            detail_text: None,
        }),
        queued_lines: Vec::new(),
        task_lines: task_lines.into_iter().map(str::to_string).collect(),
        compact_progress: None,
        background_processes_active: 0,
    }
}

fn live_status_with_queue(queued_lines: Vec<&str>) -> LiveStatusViewModel {
    LiveStatusViewModel {
        spinner: None,
        queued_lines: queued_lines.into_iter().map(str::to_string).collect(),
        task_lines: Vec::new(),
        compact_progress: None,
        background_processes_active: 0,
    }
}

#[test]
fn test_render_maps_task_status_lines_for_selection() {
    let mut output = OutputArea::new();
    let live_status = live_status(vec!["━━ Tasks: 0/1 ━━", "□ 修复 bug"]);
    let area = Rect::new(0, 0, 40, 5);
    let mut buf = Buffer::empty(area);

    output.render(area, &mut buf, &Default::default(), &live_status);
    // screen_line_map: spinner(1,不可选) + task_status(2) = 3
    assert_eq!(output.screen_line_map.len(), 3);
    // spinner 在 index 0, 不可选(usize::MAX)
    assert_eq!(output.screen_line_map[0].0, usize::MAX);
    let base = output.document().total_lines();
    assert_eq!(output.screen_line_map[1].0, base);
    assert_eq!(output.screen_line_map[2].0, base + 1);
    assert_eq!(live_status.task_lines.len(), 2);
    // rel_row=2 对应第 2 个 task_status 行
    let s = output.screen_to_anchor(2, 0, &area, &live_status).unwrap();
    let e = output.screen_to_anchor(2, 15, &area, &live_status).unwrap();
    let view = output_selection_view_for_test(s, e);

    assert_eq!(
        output.selected_text_for_view(&view, &live_status),
        Some("  □ 修复 bug".to_string())
    );
}

#[test]
fn test_render_highlights_selected_task_status_line() {
    let mut output = OutputArea::new();
    let live_status = live_status(vec!["□ 修复 bug"]);
    let area = Rect::new(0, 0, 40, 4);
    let mut buf = Buffer::empty(area);

    output.render(area, &mut buf, &Default::default(), &live_status);
    // screen_map: [spinner(usize::MAX), task_status(lines.len())]
    // 选 task_status 行（screen 行 1）
    let s = output.screen_to_anchor(1, 0, &area, &live_status).unwrap();
    let e = output.screen_to_anchor(1, 8, &area, &live_status).unwrap();
    let view = output_selection_view_for_test(s, e);
    output.render(area, &mut buf, &view, &live_status);

    let first_selected = buf.cell((area.x, area.y + 1)).unwrap();
    assert_eq!(first_selected.style().bg, Some(theme::SELECTION_BG));
    assert_eq!(first_selected.style().fg, Some(theme::SELECTION_FG));

    let unselected = buf.cell((area.x + 9, area.y + 1)).unwrap();
    assert_ne!(unselected.style().bg, Some(theme::SELECTION_BG));
}

#[test]
fn test_render_preserves_queued_input_hard_newlines() {
    let mut output = OutputArea::new();
    let live_status = live_status_with_queue(vec!["> alpha", "  beta"]);
    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);

    output.render(area, &mut buf, &Default::default(), &live_status);

    assert_eq!(buf.cell((0, 0)).unwrap().symbol(), ">");
    assert_eq!(buf.cell((2, 0)).unwrap().symbol(), "a");
    assert_eq!(buf.cell((2, 1)).unwrap().symbol(), "b");
}

#[test]
fn test_render_wraps_long_queued_input_lines() {
    let mut output = OutputArea::new();
    let live_status = live_status_with_queue(vec!["> abcdef"]);
    let area = Rect::new(0, 0, 6, 3);
    let mut buf = Buffer::empty(area);

    output.render(area, &mut buf, &Default::default(), &live_status);

    assert_eq!(buf.cell((0, 0)).unwrap().symbol(), ">");
    assert_eq!(buf.cell((3, 0)).unwrap().symbol(), "b");
    assert_eq!(buf.cell((2, 1)).unwrap().symbol(), "c");
}

// ── #1895：后台进程计数 status line 第三行 ────────────────────────

#[test]
fn background_process_count_renders_third_line_with_spinner() {
    let mut output = OutputArea::new();
    let mut status = live_status(vec!["━━ Tasks: 0/1 ━━"]);
    status.background_processes_active = 2;
    let area = Rect::new(0, 0, 40, 8);
    let mut buf = Buffer::empty(area);
    output.render(area, &mut buf, &Default::default(), &status);
    let text = buffer_text(&buf);
    assert!(
        text.contains("2 Backend Progress"),
        "有 spinner 时计数仍显示（第三行）：{text}"
    );
}

#[test]
fn background_process_count_visible_without_spinner() {
    let mut output = OutputArea::new();
    let mut status = live_status_with_queue(vec![]);
    status.background_processes_active = 3;
    let area = Rect::new(0, 0, 40, 8);
    let mut buf = Buffer::empty(area);
    output.render(area, &mut buf, &Default::default(), &status);
    let text = buffer_text(&buf);
    assert!(
        text.contains("3 Backend Progress"),
        "无 active Run（spinner 消失）计数仍可见：{text}"
    );
}

#[test]
fn background_process_count_hidden_when_zero() {
    let mut output = OutputArea::new();
    let status = live_status_with_queue(vec![]);
    let area = Rect::new(0, 0, 40, 8);
    let mut buf = Buffer::empty(area);
    output.render(area, &mut buf, &Default::default(), &status);
    let text = buffer_text(&buf);
    assert!(
        !text.contains("Backend Progress"),
        "计数 0 不显示第三行：{text}"
    );
}

fn buffer_text(buf: &Buffer) -> String {
    buf.content
        .iter()
        .map(|cell| cell.symbol().to_string())
        .collect()
}
