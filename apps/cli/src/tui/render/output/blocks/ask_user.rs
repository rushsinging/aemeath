//! AskUserQuestion 交互块渲染：问题 + 操作提示 + 选项列表，当前选项高亮。
//!
//! 高亮（cursor / multi_select 勾选）由 view model 的 `cursor`/`selected` 决定，
//! 属于「选项导航高亮」，与文本选区 overlay 无关。

use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::AskUserBlockView;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

/// 渲染单个选项的行（支持多行选项文本）。
fn option_lines(index: usize, option: &str, active: bool, multi_select: bool) -> Vec<String> {
    let prefix = if multi_select {
        let check = if active { "✓" } else { " " };
        format!("  [{check}] {}. ", index + 1)
    } else {
        let marker = if active { "❯" } else { " " };
        format!("  {marker} {}. ", index + 1)
    };
    let continuation = " ".repeat(prefix.chars().count());
    let parts: Vec<&str> = option.lines().collect();
    if parts.is_empty() {
        return vec![prefix];
    }
    parts
        .iter()
        .enumerate()
        .map(|(line_idx, part)| {
            if line_idx == 0 {
                format!("{prefix}{part}")
            } else {
                format!("{continuation}{part}")
            }
        })
        .collect()
}

pub fn render_ask_user(
    block_id: &str,
    view: &AskUserBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let header_style = Style::default()
        .fg(theme::WARNING)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(theme::TEXT_DIM);
    let normal_style = Style::default().fg(theme::TEXT);

    let mut lines = Vec::new();
    lines.push(RenderedLine::new(vec![Span::styled(
        "━━ 需要你的回答 ━━".to_string(),
        header_style,
    )]));
    lines.push(RenderedLine::new(vec![Span::raw("")]));
    for line in view.question.lines() {
        lines.push(RenderedLine::new(vec![Span::styled(
            line.to_string(),
            header_style,
        )]));
    }

    if view.options.is_empty() {
        lines.push(RenderedLine::new(vec![Span::raw("")]));
        lines.push(RenderedLine::new(vec![Span::styled(
            "  [Enter] 确认  [Esc] 取消".to_string(),
            hint_style,
        )]));
        return RenderedBlock {
            block_id: block_id.to_string(),
            lines,
        };
    }

    let hint = if view.multi_select {
        "  [↑↓] 移动  [Space] 选中/取消  [Enter] 确认  [Esc] 取消"
    } else {
        "  [↑↓] 选择  [Enter] 确认  [Esc] 取消"
    };
    lines.push(RenderedLine::new(vec![Span::styled(
        hint.to_string(),
        hint_style,
    )]));
    lines.push(RenderedLine::new(vec![Span::raw("")]));

    for (i, option) in view.options.iter().enumerate() {
        // chat_input_active 子态下不高亮任何选项
        let is_cursor = !view.chat_input_active && i == view.cursor;
        let is_checked = view.multi_select && view.selected.get(i).copied().unwrap_or(false);
        let active = is_cursor || is_checked;
        for (line_idx, content) in option_lines(i, option, active, view.multi_select)
            .into_iter()
            .enumerate()
        {
            let style = if active && line_idx == 0 {
                header_style
            } else {
                normal_style
            };
            lines.push(RenderedLine::new(vec![Span::styled(content, style)]));
        }
    }

    lines.push(RenderedLine::new(vec![Span::raw("")]));

    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(options: &[&str], cursor: usize, multi: bool) -> AskUserBlockView {
        AskUserBlockView {
            key: "ask".into(),
            question: "选哪个?".into(),
            options: options.iter().map(|s| s.to_string()).collect(),
            llm_option_count: options.len(),
            multi_select: multi,
            cursor,
            selected: vec![false; options.len()],
            chat_input_active: false,
        }
    }

    #[test]
    fn test_render_ask_user_highlights_cursor_option() {
        let block = render_ask_user("ask", &view(&["A", "B"], 1, false), &RenderCtx { width: 80 });
        // 找到选项 B 行（含 ❯ 标记表示高亮）
        let highlighted = block
            .lines
            .iter()
            .find(|line| line.plain.contains("2. B"))
            .expect("option B line");
        assert!(highlighted.plain.contains('❯'));
        assert_eq!(highlighted.spans[0].style.fg, Some(theme::WARNING));
        // 选项 A 不应高亮
        let other = block
            .lines
            .iter()
            .find(|line| line.plain.contains("1. A"))
            .expect("option A line");
        assert!(!other.plain.contains('❯'));
    }

    #[test]
    fn test_render_ask_user_empty_options_shows_confirm_hint() {
        let mut v = view(&[], 0, false);
        v.options.clear();
        v.llm_option_count = 0;
        let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
        assert!(block
            .lines
            .iter()
            .any(|line| line.plain.contains("[Enter] 确认")));
        // 无选项时不渲染 ↑↓ 选择提示
        assert!(!block
            .lines
            .iter()
            .any(|line| line.plain.contains("[↑↓]")));
    }

    #[test]
    fn test_render_ask_user_single_option_renders_marker() {
        let block = render_ask_user("ask", &view(&["Only"], 0, false), &RenderCtx { width: 80 });
        let line = block
            .lines
            .iter()
            .find(|line| line.plain.contains("1. Only"))
            .expect("only option");
        assert!(line.plain.contains('❯'));
    }

    #[test]
    fn test_render_ask_user_multi_select_shows_checkbox() {
        let mut v = view(&["A", "B"], 0, true);
        v.selected = vec![false, true];
        let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
        let checked = block
            .lines
            .iter()
            .find(|line| line.plain.contains("2. B"))
            .expect("option B");
        assert!(checked.plain.contains("[✓]"));
    }

    #[test]
    fn test_render_ask_user_chat_input_active_suppresses_highlight() {
        let mut v = view(&["A", "B"], 0, false);
        v.chat_input_active = true;
        let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
        // chat 子态下无任何 ❯ 高亮
        assert!(!block.lines.iter().any(|line| line.plain.contains('❯')));
    }

    #[test]
    fn test_option_lines_multiline_indents_continuation() {
        let rendered = option_lines(0, "first\nsecond", false, false);
        assert_eq!(rendered.len(), 2);
        assert!(rendered[0].contains("1. first"));
        assert!(rendered[1].trim() == "second");
    }
}
